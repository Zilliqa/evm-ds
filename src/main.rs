use clap::Parser;
use jsonrpc_ipc_server::ServerBuilder;
use jsonrpc_ipc_server::jsonrpc_core::*;
use jsonrpc_core::Result;
use jsonrpc_derive::rpc;


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
}

#[rpc]
pub trait Rpc {
    #[rpc(name = "run")]
    fn run(&self, account: H160, code: &[u8]) -> Result<()>;
}

pub struct RpcImpl;
impl Rpc for RpcImpl {
    fn run(&self, code: &[u8]) -> Result<()> {
        Ok(())
    }
}

fn main() {
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
    io.add_method("say_hello", |_params| async {
	Ok(Value::String("hello".into()))
    });

    io.add_method("call", call::run);
    
    let builder = ServerBuilder::new(io);
    let server = builder.start(&args.socket).expect("Couldn't open socket");
    server.wait();

}
