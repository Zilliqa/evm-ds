use std::cell::{Ref, RefCell};
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

fn query_jsonrpc_u64<T: From<u64>>(client: &RawClient, query_name: &str) -> T {
    let mut args = serde_json::Map::new();
    args.insert("query_name".into(), query_name.into());
    args.insert("query_args".into(), "".into());
    let result: Value = futures::executor::block_on(async move {
        client
            .call_method("fetchBlockchainInfo", Params::Map(args))
            .await
    })
    .expect("fetchBlockchainInfo call");
    result
        .get(1)
        .expect("fetchBlockchainInfo result")
        .as_u64()
        .expect("fetchBlockchainInfo BLOCKNUMBER")
        .into()
}

impl<'config> Backend for ScillaBackend {
    fn gas_price(&self) -> U256 {
        // self.backend.gas_price()
        U256::from(2_000_000_000) // see constants.xml in the Zilliqa codebase.
    }
    fn origin(&self) -> H160 {
        H160::zero()
        // self.backend.origin()
    }
    fn block_hash(&self, _number: U256) -> H256 {
        H256::zero()
        // self.backend.block_hash(number)
    }
    fn block_number(&self) -> U256 {
        query_jsonrpc_u64(&self.client(), "BLOCKNUMBER")
    }
    fn block_coinbase(&self) -> H160 {
        H160::zero()
        // self.backend.block_coinbase()
    }
    fn block_timestamp(&self) -> U256 {
        query_jsonrpc_u64(&self.client(), "TIMESTAMP")
    }
    fn block_difficulty(&self) -> U256 {
        U256::one()
        // self.backend.block_difficulty()
    }
    fn block_gas_limit(&self) -> U256 {
        U256::one()
        // self.backend.block_gas_limit()
    }
    fn block_base_fee_per_gas(&self) -> U256 {
        U256::one()
        // self.backend.block_base_fee_per_gas()
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
