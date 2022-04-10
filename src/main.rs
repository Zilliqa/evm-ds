//! Implementation of EVM for Zilliqa

// #![deny(warnings)]
#![forbid(unsafe_code)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::rc::Rc;

use clap::Parser;
use evm::executor::stack::{MemoryStackState, StackSubstateMetadata};
use evm::tracing;

use core::str::FromStr;
use log::{debug, info};

use jsonrpc_core::{Error, IoHandler, Result};
use jsonrpc_derive::rpc;
use jsonrpc_ipc_server;
use primitive_types::*;
use scillabackend::ScillaBackend;

use tokio;

mod scillabackend;

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

    /// Path of the EVM server Unix domain socket.
    #[clap(short = 'p', long, default_value = "3333")]
    http_port: u16,

    /// Trace the execution with debug logging.
    #[clap(short, long)]
    tracing: bool,
}

#[rpc(server)]
pub trait Rpc {
    #[rpc(name = "run")]
    fn run(
        &self,
        address: String,
        caller: String,
        code: String,
        data: String,
        apparent_value: String,
    ) -> Result<evm::ExitReason>;
}

pub struct EvmServer {
    tracing: bool,
    backend: ScillaBackend,
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
    ) -> Result<evm::ExitReason> {
        let code =
            Rc::new(hex::decode(&code_hex).map_err(|e| Error::invalid_params(e.to_string()))?);
        let data =
            Rc::new(hex::decode(&data_hex).map_err(|e| Error::invalid_params(e.to_string()))?);

        let config = evm::Config::london();
        let context = evm::Context {
            address: H160::from_str(&address).map_err(|e| Error::invalid_params(e.to_string()))?,
            caller: H160::from_str(&caller).map_err(|e| Error::invalid_params(e.to_string()))?,
            apparent_value: U256::from_str(&apparent_value).map_err(|e| Error::invalid_params(e.to_string()))?,
        };
        let mut runtime = evm::Runtime::new(code, data, context, &config);
        let metadata = StackSubstateMetadata::new(GAS_LIMIT, &config);
        let state = MemoryStackState::new(metadata, &self.backend);

        // TODO: replace with the real precompiles
        let precompiles = ();

        let mut executor =
            evm::executor::stack::StackExecutor::new_with_precompiles(state, &config, &precompiles);

        info!(
            "Executing runtime with code \"{:?}\" and data \"{:?}\"",
            code_hex, data_hex,
        );
        let mut listener = LoggingEventListener;
        let exit_reason = if self.tracing {
            evm::tracing::using(&mut listener, || executor.execute(&mut runtime))
        } else {
            executor.execute(&mut runtime)
        };
        info!("Exit: {:?}", exit_reason);
        Ok(exit_reason)
    }
}

struct LoggingEventListener;

impl tracing::EventListener for LoggingEventListener {
    fn event(&mut self, event: tracing::Event) {
        debug!("{:?}", event);
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
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
    // io.add_method("say_hello", |_params| async {
    //     Ok(Value::String("hello".into()))
    // });

    // Connect to the backend as needed.
    let scilla_backend = ScillaBackend::new(args.node_socket);
    let evm_sever = EvmServer {
        tracing: args.tracing,
        backend: scilla_backend,
    };

    io.extend_with(evm_sever.to_delegate());
    let builder = jsonrpc_ipc_server::ServerBuilder::new(io.clone());
    let ipc_server = builder.start(&args.socket).expect("Couldn't open socket");
    let builder = jsonrpc_http_server::ServerBuilder::new(io);
    let http_server = builder
        .start_http(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            args.http_port,
        ))
        .expect("Couldn't open socket");

    ipc_server.wait();
    http_server.wait();

    Ok(())
}
