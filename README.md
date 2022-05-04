# EVM-DS is an EVM implementation for Zilliqa

EVM-DS is designed to run on Zilliqa Directory Service nodes.

## Building it

Install the Rust toolchain, then do:

```
cargo build --release
```

As in all Rust projects, the binary will be found in `target/release/evm-ds`, (unless the cargo configuration is changed locally, then according to the configuration).

## Running it


Running EVM-DS is similar to the Scilla interpreter. The Zilliqa node should run `evm-ds` as a subprocess.

Arguments:

  * `--socket`: Path of the EVM server Unix domain socket. The `evm-ds` binary will be the server listening on this socket and accepting EVM code execution requests on it. Default is `/tmp/evm-server.sock`.
  
  * `--node_socket`: Path of the Node Unix domain socket. The `evm-ds` binary will be the client requesting account and state data from the Zilliqa node. Default is `/tmp/zilliqa.sock`.

  * `--http_port`: an HTTP port serving the same purpose as the `--socket` above. It is needed only for debugging of `evm-ds`, as there are way more tools for HTTP JSON-RPC, than for Unix sockets.
  
  * `--tracing`: if true, additional trace logging will be enabled.
  

