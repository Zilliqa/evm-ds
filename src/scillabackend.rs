use std::{cell::{Ref, RefCell}, str::FromStr};
use std::mem;
/// Backend implementation that stores EVM state via the Scilla JSONRPC interface.
use std::path::{Path, PathBuf};

use evm::backend::{Backend, Basic};
use jsonrpc_core::serde_json;
use jsonrpc_core::{Params, Value};
use jsonrpc_core_client::{transports::ipc, RawClient, RpcError};
use primitive_types::{H160, H256, U256};

pub struct ScillaBackendFactory {
    pub path: PathBuf,
}

impl ScillaBackendFactory {
    pub fn new_backend(&self) -> ScillaBackend {
        ScillaBackend::new(&self.path)
    }
}

// Backend relying on Scilla variables and Scilla JSONRPC interface.
pub struct ScillaBackend {
    // Path to the Unix domain socket over which we talk to the Node.
    path: PathBuf,

    // Established JSONRPC client.
    client: RefCell<Option<RawClient>>,
}

impl ScillaBackend {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            client: RefCell::new(None),
        }
    }

    // Create a JSON RPC client to the node, or reuse an existing one.
    pub fn client(&self) -> Ref<'_, RawClient> {
        let chan = self.client.borrow();
        if chan.is_some() {
            return Ref::map(chan, |x| x.as_ref().unwrap());
        }
        let client = futures::executor::block_on(async {
            let client = ipc::connect(&self.path).await?;
            Result::<RawClient, RpcError>::Ok(client)
        })
        .expect("Node JSONRPC client");
        *self.client.borrow_mut() = Some(client);
        Ref::map(self.client.borrow(), |x| x.as_ref().unwrap())
    }
}

fn query_jsonrpc(
    client: &RawClient,
    query_name: &str,
    query_args: Option<&str>,
) -> Value {
    // Make a JSON Query for fetchBlockchaininfo
    let mut args = serde_json::Map::new();
    args.insert("query_name".into(), query_name.into());
    args.insert("query_args".into(), query_args.unwrap_or_default().into());
    let mut result: Value = futures::executor::block_on(async move {
        client
            .call_method("fetchBlockchainInfo", Params::Map(args))
            .await
    })
    .expect("fetchBlockchainInfo call");

    // Check that the call succeeded.
    assert_eq!(
        true,
        result
            .get(0)
            .expect("fetchBlockchainInfo result")
            .as_bool()
            .expect("fetchBlockchainInfo result")
    );

    // Check that there is a result of a given type.
    let result = result.get_mut(1).expect("fetchBlockchainInfo result");
    mem::replace(result, Value::default())
}

fn query_jsonrpc_u64<OutputType: From<u64>>(client: &RawClient, query_name: &str) -> OutputType {
    serde_json::from_value::<u64>(query_jsonrpc(client, query_name, None))
        .expect("fetchBlockchainInfo BLOCKNUMBER")
        .into()
}

impl<'config> Backend for ScillaBackend {
    fn gas_price(&self) -> U256 {
        U256::from(2_000_000_000) // see constants.xml in the Zilliqa codebase.
    }

    fn origin(&self) -> H160 {
        let result = query_jsonrpc(&self.client(), "ORIGIN", None);
        H160::from_str(result.as_str().expect("origin")).expect("origin hex")
    }

    fn block_hash(&self, number: U256) -> H256 {
        let result = query_jsonrpc(&self.client(), "BLOCKHASH", Some(&number.to_string()));
        H256::from_str(result.as_str().expect("blockhash")).expect("blockhash hex")
    }

    fn block_number(&self) -> U256 {
        query_jsonrpc_u64(&self.client(), "BLOCKNUMBER")
    }

    fn block_coinbase(&self) -> H160 {
        H160::zero()
    }

    fn block_timestamp(&self) -> U256 {
        query_jsonrpc_u64(&self.client(), "TIMESTAMP")
    }

    fn block_difficulty(&self) -> U256 {
        query_jsonrpc_u64(&self.client(), "BLOCKDIFFICULTY")
    }

    fn block_gas_limit(&self) -> U256 {
        query_jsonrpc_u64(&self.client(), "BLOCKGASLIMIT")
    }

    fn block_base_fee_per_gas(&self) -> U256 {
        self.gas_price()
    }

    fn chain_id(&self) -> U256 {
        let base_chain_id = 33000u64;
        let chain_id: u64 = query_jsonrpc_u64(&self.client(), "CHAINID");
        (chain_id + base_chain_id).into()
    }

    fn exists(&self, _address: H160) -> bool {
        // self.substate.known_account(address).is_some() || self.backend.exists(address)
        false
    }

    fn basic(&self, _address: H160) -> Basic {
        // self.substate
        //     .known_basic(address)
        //     .unwrap_or_else(|| self.backend.basic(address))
        Basic {
            balance: U256::zero(),
            nonce: U256::zero(),
        }
    }

    fn code(&self, _address: H160) -> Vec<u8> {
        vec![0, 1, 2, 3, 4]
        // self.substate
        //     .known_code(address)
        //     .unwrap_or_else(|| self.backend.code(address))
    }

    fn storage(&self, _address: H160, _key: H256) -> H256 {
        H256::zero()
        // self.substate
        //     .known_storage(address, key)
        //     .unwrap_or_else(|| self.backend.storage(address, key))
    }

    fn original_storage(&self, _address: H160, _key: H256) -> Option<H256> {
        Some(H256::zero())
        // if let Some(value) = self.substate.known_original_storage(address, key) {
        //     return Some(value);
        // }

        // self.backend.original_storage(address, key)
    }
}
