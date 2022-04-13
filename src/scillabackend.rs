use std::mem;
/// Backend implementation that stores EVM state via the Scilla JSONRPC interface.
use std::path::{Path, PathBuf};
use std::{
    cell::{Ref, RefCell},
    str::FromStr,
};

use evm::backend::{Backend, Basic};
use jsonrpc_core::serde_json;
use jsonrpc_core::{Params, Value};
use jsonrpc_core_client::{transports::ipc, RawClient, RpcError};
use primitive_types::{H160, H256, U256};

use protobuf::Message;

use crate::protos::ScillaMessage;

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
    fn client(&self) -> Ref<'_, RawClient> {
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

    // Call the Scilla IPC Server API.
    fn call_ipc_server_api(&self, method: &str, args: serde_json::Map<String, Value>) -> Value {
        futures::executor::block_on(async move {
            self.client().call_method(method, Params::Map(args)).await
        })
        .expect(&format!("{} call", method))
    }

    fn query_jsonrpc(&self, query_name: &str, query_args: Option<&str>) -> Value {
        // Make a JSON Query for fetchBlockchaininfo
        let mut args = serde_json::Map::new();
        args.insert("query_name".into(), query_name.into());
        args.insert("query_args".into(), query_args.unwrap_or_default().into());
        let mut result = self.call_ipc_server_api("fetchBlockchainInfo", args);
        // Check that the call succeeded.
        assert!(result
            .get(0)
            .expect("fetchBlockchainInfo result")
            .as_bool()
            .expect("fetchBlockchainInfo result"));

        // Check that there is a result of a given type.
        let result = result.get_mut(1).expect("fetchBlockchainInfo result");
        mem::take(result)
    }

    fn query_jsonrpc_u64<OutputType: From<u64>>(&self, query_name: &str) -> OutputType {
        serde_json::from_value::<u64>(self.query_jsonrpc(query_name, None))
            .expect("fetchBlockchainInfo BLOCKNUMBER")
            .into()
    }
}

impl<'config> Backend for ScillaBackend {
    fn gas_price(&self) -> U256 {
        U256::from(2_000_000_000) // see constants.xml in the Zilliqa codebase.
    }

    fn origin(&self) -> H160 {
        let result = self.query_jsonrpc("ORIGIN", None);
        H160::from_str(result.as_str().expect("origin")).expect("origin hex")
    }

    fn block_hash(&self, number: U256) -> H256 {
        let result = self.query_jsonrpc("BLOCKHASH", Some(&number.to_string()));
        H256::from_str(result.as_str().expect("blockhash")).expect("blockhash hex")
    }

    fn block_number(&self) -> U256 {
        self.query_jsonrpc_u64("BLOCKNUMBER")
    }

    fn block_coinbase(&self) -> H160 {
        H160::zero()
    }

    fn block_timestamp(&self) -> U256 {
        self.query_jsonrpc_u64("TIMESTAMP")
    }

    fn block_difficulty(&self) -> U256 {
        self.query_jsonrpc_u64("BLOCKDIFFICULTY")
    }

    fn block_gas_limit(&self) -> U256 {
        self.query_jsonrpc_u64("BLOCKGASLIMIT")
    }

    fn block_base_fee_per_gas(&self) -> U256 {
        self.gas_price()
    }

    fn chain_id(&self) -> U256 {
        let base_chain_id = 33000u64;
        let chain_id: u64 = self.query_jsonrpc_u64("CHAINID");
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

    fn code(&self, address: H160) -> Vec<u8> {
        let mut query = ScillaMessage::ProtoScillaQuery::new();
        query.set_name("_code".into());
        query.set_mapdepth(0);

        let mut args = serde_json::Map::new();
        args.insert("addr".into(), hex::encode(address.as_bytes()).into());
        args.insert("query".into(), query.write_to_bytes().unwrap().into());

        // If the RPC call failed, something is wrong, and it is better to crash.
        let mut result = self.call_ipc_server_api("fetchExternalStateValue", args);
        // If the RPC was okay, but we didn't get a value, that's
        // normal, just return empty code.
        if !result
            .get(0)
            .map(|x| x.as_bool().unwrap_or_default())
            .unwrap_or_default()
        {
            return Vec::new();
        }
        // Check that there is a result of a given type.
        let mut default_value = Value::String("".into());
        let result = result.get_mut(1).unwrap_or(&mut default_value);
        let result = mem::take(result);
        let result = result.as_str().unwrap_or("").to_string();
        result.into_bytes()
    }

    fn storage(&self, address: H160, key: H256) -> H256 {
        let mut query = ScillaMessage::ProtoScillaQuery::new();
        query.set_name("_evm_storage".into());
        query.set_indices(vec![bytes::Bytes::from(key.as_bytes().to_vec())]);
        query.set_mapdepth(1);

        let mut args = serde_json::Map::new();
        args.insert("addr".into(), hex::encode(address.as_bytes()).into());
        args.insert("query".into(), query.write_to_bytes().unwrap().into());

        // If the RPC call failed, something is wrong, and it is better to crash.
        let mut result = self.call_ipc_server_api("fetchExternalStateValue", args);
        // If the RPC was okay, but we didn't get a value, that's
        // normal, just return zero.
        if !result
            .get(0)
            .map(|x| x.as_bool().unwrap_or_default())
            .unwrap_or_default()
        {
            return H256::zero();
        }

        // Check that there is a result of a given type.
        let mut default_value = Value::String("0".to_string());
        let result = result.get_mut(1).unwrap_or(&mut default_value);
        let result = mem::take(result);
        let result = result.as_str().unwrap_or("0");
        let result = hex::decode(result).unwrap_or(vec![0u8]);
        H256::from_slice(&result)
    }

    // We implement original_storage via storage, as we postpone writes until
    // contract commit time.
    fn original_storage(&self, address: H160, key: H256) -> Option<H256> {
        Some(self.storage(address, key))
    }
}
