//! Implementation of EVM for Zilliqa

// #![deny(warnings)]
#![forbid(unsafe_code)]

mod ipc_connect;
mod protos;
mod scillabackend;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use clap::Parser;
use evm::{
    backend::Apply,
    executor::stack::{MemoryStackState, StackSubstateMetadata},
    tracing,
};

use serde::ser::{Serialize, SerializeStructVariant, Serializer};

use core::str::FromStr;
use log::{debug, info};

use jsonrpc_core::{BoxFuture, Error, ErrorCode, IoHandler, Result};
use jsonrpc_derive::rpc;
use jsonrpc_server_utils::codecs;
use primitive_types::*;
use scillabackend::{ScillaBackend, ScillaBackendFactory};
use tokio::runtime::Handle;

/// EVM JSON-RPC server
#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Args {
    /// Path of the EVM server Unix domain socket.
    #[clap(short, long, default_value = "/tmp/evm-server.sock")]
    socket: String,

    /// Path of the Node Unix domain socket.
    #[clap(short, long, default_value = "/tmp/zilliqa.sock")]
    node_socket: String,

    /// Path of the EVM server HTTP socket. Duplicates the `socket` above for convenience.
    #[clap(short = 'p', long, default_value = "3333")]
    http_port: u16,

    /// Trace the execution with debug logging.
    #[clap(short, long)]
    tracing: bool,
}

struct DirtyState(Apply<Vec<(H256, H256)>>);

impl Serialize for DirtyState {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.0 {
            Apply::Modify {
                ref address,
                ref basic,
                ref code,
                ref storage,
                reset_storage,
            } => {
                let mut state = serializer.serialize_struct_variant("A", 0, "modify", 6)?;
                state.serialize_field("address", address)?;
                state.serialize_field("balance", &basic.balance)?;
                state.serialize_field("nonce", &basic.nonce)?;
                state.serialize_field("code", code)?;
                state.serialize_field("storage", storage)?;
                state.serialize_field("reset_storage", &reset_storage)?;
                Ok(state.end()?)
            }
            Apply::Delete { address } => {
                let mut state = serializer.serialize_struct_variant("A", 0, "delete", 1)?;
                state.serialize_field("address", address)?;
                Ok(state.end()?)
            }
        }
    }
}

#[derive(serde::Serialize)]
pub struct EvmResult {
    exit_reason: evm::ExitReason,
    return_value: String,
    apply: Vec<DirtyState>,
    logs: Vec<ethereum::Log>,
}

#[rpc(server)]
pub trait Rpc: Send + 'static {
    #[rpc(name = "run")]
    fn run(
        &self,
        address: String,
        caller: String,
        code: String,
        data: String,
        apparent_value: String,
    ) -> BoxFuture<Result<EvmResult>>;
}

struct EvmServer {
    tracing: bool,
    backend_factory: ScillaBackendFactory,
}

// TODO: remove this and introduce gas limit calculation based on balance etc.
const GAS_LIMIT: u64 = 1_000_000_000;

impl Rpc for EvmServer {
    fn run(
        &self,
        address: String,
        caller: String,
        code_hex: String,
        data_hex: String,
        apparent_value: String,
    ) -> BoxFuture<Result<EvmResult>> {
        let backend = self.backend_factory.new_backend();
        let tracing = self.tracing;
        Box::pin(async move {
            run_evm_impl(
                address,
                caller,
                code_hex,
                data_hex,
                apparent_value,
                backend,
                tracing,
            )
            .await
        })
    }
}

async fn run_evm_impl(
    address: String,
    caller: String,
    code_hex: String,
    data_hex: String,
    apparent_value: String,
    backend: ScillaBackend,
    tracing: bool,
) -> Result<EvmResult> {
    tokio::task::spawn_blocking(move || {
        let code =
            Rc::new(hex::decode(&code_hex).map_err(|e| Error::invalid_params(e.to_string()))?);
        let data =
            Rc::new(hex::decode(&data_hex).map_err(|e| Error::invalid_params(e.to_string()))?);

        let config = evm::Config::london();
        let context = evm::Context {
            address: H160::from_str(&address).map_err(|e| Error::invalid_params(e.to_string()))?,
            caller: H160::from_str(&caller).map_err(|e| Error::invalid_params(e.to_string()))?,
            apparent_value: U256::from_str(&apparent_value)
                .map_err(|e| Error::invalid_params(e.to_string()))?,
        };
        let mut runtime = evm::Runtime::new(code, data, context, &config);
        let metadata = StackSubstateMetadata::new(GAS_LIMIT, &config);
        let state = MemoryStackState::new(metadata, &backend);

        // TODO: replace with the real precompiles
        let precompiles = ();

        let mut executor =
            evm::executor::stack::StackExecutor::new_with_precompiles(state, &config, &precompiles);

        info!(
            "Executing runtime with code \"{:?}\" and data \"{:?}\"",
            code_hex, data_hex,
        );
        let mut listener = LoggingEventListener;

        // We have to catch panics, as error handling in the Backend interface of
        // do not have Result, assuming all operations are successful.
        //
        // We are asserting it is safe to unwind, as objects will be dropped after
        // the unwind.
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            if tracing {
                evm::tracing::using(&mut listener, || executor.execute(&mut runtime))
            } else {
                executor.execute(&mut runtime)
            }
        }));
        match result {
            Ok(exit_reason) => {
                info!("Exit: {:?}", exit_reason);

                let (state_apply, logs) = executor.into_state().deconstruct();
                Ok(EvmResult {
                    exit_reason,
                    return_value: hex::encode(runtime.machine().return_value()),
                    apply: state_apply
                        .into_iter()
                        .map(|apply| match apply {
                            Apply::Delete { address } => DirtyState(Apply::Delete { address }),
                            Apply::Modify {
                                address,
                                basic,
                                code,
                                storage,
                                reset_storage,
                            } => DirtyState(Apply::Modify {
                                address,
                                basic,
                                code,
                                storage: storage.into_iter().collect(),
                                reset_storage,
                            }),
                        })
                        .collect(),
                    logs: logs.into_iter().collect(),
                })
            }
            Err(_) => Err(Error {
                code: ErrorCode::InternalError,
                message: "EVM execution failed".to_string(),
                data: None,
            }),
        }
    })
    .await
    .unwrap()
}

struct LoggingEventListener;

impl tracing::EventListener for LoggingEventListener {
    fn event(&mut self, event: tracing::Event) {
        debug!("{:?}", event);
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .parse_env("EVM_LOG")
        .init();

    let args = Args::parse();

    // Required methods:
    // - check (_json - wtf is the parameter?)
    // - run (_json)
    // - disambiguate (_json)

    let mut io = IoHandler::new();
    // Connect to the backend as needed.
    let evm_sever = EvmServer {
        tracing: args.tracing,
        backend_factory: ScillaBackendFactory {
            path: PathBuf::from(args.node_socket),
        },
    };

    let tokio_runtime_handle = Handle::current();

    io.extend_with(evm_sever.to_delegate());
    let ipc_server_handle: Arc<Mutex<Option<jsonrpc_ipc_server::CloseHandle>>> =
        Arc::new(Mutex::new(None));
    let ipc_server_handle_clone = ipc_server_handle.clone();
    let http_server_handle: Arc<Mutex<Option<jsonrpc_http_server::CloseHandle>>> =
        Arc::new(Mutex::new(None));
    let http_server_handle_clone = http_server_handle.clone();
    io.add_method("die", move |_params| {
        if let Some(handle) = ipc_server_handle_clone.lock().unwrap().take() {
            handle.close()
        }
        if let Some(handle) = http_server_handle_clone.lock().unwrap().take() {
            handle.close()
        }
        futures::future::ready(Ok(jsonrpc_core::Value::Null))
    });

    // Start the IPC server (Unix domain socket).
    let builder = jsonrpc_ipc_server::ServerBuilder::new(io.clone())
        .request_separators(
            codecs::Separator::Byte(b'\n'),
            codecs::Separator::Byte(b'\n'),
        )
        .event_loop_executor(tokio_runtime_handle.clone());
    let ipc_server = builder.start(&args.socket).expect("Couldn't open socket");
    // Save the handle so that we can shut it down gracefully.
    *ipc_server_handle.lock().unwrap() = Some(ipc_server.close_handle());

    // Start the HTTP server.
    let builder = jsonrpc_http_server::ServerBuilder::new(io)
        .event_loop_executor(tokio_runtime_handle.clone());
    let http_server = builder
        .start_http(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            args.http_port,
        ))
        .expect("Couldn't open socket");
    // Save the handle so that we can shut it down gracefully.
    *http_server_handle.lock().unwrap() = Some(http_server.close_handle());

    tokio::spawn(async move {
        ipc_server.wait();
        http_server.wait();
        println!("Dying gracefully");
    })
    .await?;

    Ok(())
}
