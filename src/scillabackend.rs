use std::mem;
/// Backend implementation that stores EVM state via the Scilla JSONRPC interface.
use std::path::{Path, PathBuf};
use std::{
    cell::{Ref, RefCell},
    str::FromStr,
};

use evm::backend::{Backend, Basic};
use jsonrpc_core::serde_json;
use jsonrpc_core::{Error, Params, Result, Value};
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
            std::result::Result::<RawClient, RpcError>::Ok(client)
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
        .unwrap_or_else(|_| panic!("{} call", method))
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

    fn query_state_value(
        &self,
        address: H160,
        query_name: &str,
        key: Option<H256>,
        use_default: bool,
    ) -> Result<Option<Value>> {
        let mut query = ScillaMessage::ProtoScillaQuery::new();
        query.set_name(query_name.into());
        if let Some(key) = key {
            query.set_indices(vec![bytes::Bytes::from(key.as_bytes().to_vec())]);
            query.set_mapdepth(1);
        } else {
            query.set_mapdepth(0);
        }

        let mut args = serde_json::Map::new();
        args.insert("addr".into(), hex::encode(address.as_bytes()).into());
        args.insert("query".into(), query.write_to_bytes().unwrap().into());

        // If the RPC call failed, something is wrong, and it is better to crash.
        let mut result = self.call_ipc_server_api("fetchExternalStateValue", args);
        // If the RPC was okay, but we didn't get a value, that's
        // normal, just return empty code.
        let default_false = Value::Bool(false);
        if !result
            .get(0)
            .map_or_else(
                || {
                    if use_default {
                        Ok(&default_false)
                    } else {
                        Err(Error::internal_error())
                    }
                },
                Ok,
            )?
            .as_bool()
            .unwrap_or_default()
        {
            return Ok(None);
        }
        // Check that there is a result of a given type.
        let mut default_value = Value::String("".into());
        let result = result.get_mut(1).map_or_else(
            || {
                if use_default {
                    Ok(&mut default_value)
                } else {
                    Err(Error::internal_error())
                }
            },
            Ok,
        )?;
        Ok(Some(mem::take(result)))
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

    fn basic(&self, address: H160) -> Basic {
        let result = self
            .query_state_value(address, "_balance", None, false)
            .expect("query_state_value")
            .map(|x| x.as_u64().expect("balance as number"))
            .unwrap_or(0);
        let balance = U256::from(result);
        let result = self
            .query_state_value(address, "_nonce", None, false)
            .expect("query_state_value")
            .map(|x| x.as_u64().expect("nonce as number"))
            .unwrap_or(0);
        let nonce = U256::from(result);
        Basic { balance, nonce }
    }

    fn code(&self, address: H160) -> Vec<u8> {
        self.query_state_value(address, "_code", None, true)
            .expect("query_state_value(_code)")
            .expect("query_state_value(_code) result")
            .as_str()
            .unwrap_or("")
            .to_string()
            .into_bytes()
    }

    fn storage(&self, address: H160, key: H256) -> H256 {
        let result = self.query_state_value(address, "_evm_storage", Some(key), true)
            .expect("query_state_value(_evm_storage)")
            .expect("query_state_value(_evm_storage) result");
        let result = result.as_str().unwrap_or("0");
        let result = hex::decode(result).unwrap_or_else(|_| vec![0u8]);
        H256::from_slice(&result)
    }

    // We implement original_storage via storage, as we postpone writes until
    // contract commit time.
    fn original_storage(&self, address: H160, key: H256) -> Option<H256> {
        Some(self.storage(address, key))
    }
}
