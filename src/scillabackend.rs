use std::mem;
/// Backend implementation that stores EVM state via the Scilla JSONRPC interface.
use std::path::{Path, PathBuf};
use std::str::FromStr;

use evm::backend::{Backend, Basic};
use jsonrpc_core::serde_json;
use jsonrpc_core::types::params::Params;
use jsonrpc_core::{Error, Result, Value};
use jsonrpc_core_client::RawClient;
use primitive_types::{H160, H256, U256};

use log::{debug, info};

use protobuf::Message;

use crate::ipc_connect;
use crate::protos::ScillaMessage;

const BASE_CHAIN_ID: u64 = 33000;

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
}

impl ScillaBackend {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    // Call the Scilla IPC Server API.
    fn call_ipc_server_api(&self, method: &str, args: serde_json::Map<String, Value>) -> Value {
        debug!("call_ipc_server_api: {}, {:?}", method, args);
        // Within this runtime, we need a separate runtime just to handle all JSON
        // client operations. The runtime will then drop and close all connections
        // and release all resources. Also when the thread panics.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let call_with_timeout = rt.block_on(async move {
            let client: RawClient = ipc_connect::ipc_connect(&self.path).await.unwrap();
            tokio::time::timeout(
                tokio::time::Duration::from_secs(2), // Require response in 2 secs max.
                client.call_method(method, Params::Map(args)),
            )
            .await
        });
        if let Ok(result) = call_with_timeout {
            result.unwrap_or_else(|e| {
                panic!("{} call, err {:?}", method, e);
            })
        } else {
            panic!("timeout calling {}", method);
        }
    }

    fn query_jsonrpc(&self, query_name: &str, query_args: Option<&str>) -> Value {
        info!("query_jsonrpc: {}, {:?}", query_name, query_args);
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
            .unwrap_or_default()
            .into()
    }

    fn query_state_value(
        &self,
        address: H160,
        query_name: &str,
        key: Option<H256>,
        use_default: bool,
    ) -> Result<Option<Value>> {
        info!(
            "query_state_value: {} {} {:?} {}",
            address, query_name, key, use_default
        );
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
        args.insert(
            "query".into(),
            base64::encode(query.write_to_bytes().unwrap()).into(),
        );

        // If the RPC call failed, something is wrong, and it is better to crash.
        let mut result = self.call_ipc_server_api("fetchExternalStateValueB64", args);
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

    // Encode key/value pairs for storage in such a way that the Zilliqa node
    // could interpret it without much modification.
    pub(crate) fn encode_storage(&self, key: H256, value: H256) -> (String, String) {
        let mut query = ScillaMessage::ProtoScillaQuery::new();
        query.set_name("_evm_storage".into());
        query.set_indices(vec![bytes::Bytes::from(key.as_bytes().to_vec())]);
        query.set_mapdepth(1);
        let mut val = ScillaMessage::ProtoScillaVal::new();
        let bval = value.as_bytes().to_vec();
        val.set_bval(bval.into());
        (base64::encode(query.write_to_bytes().unwrap()),
         base64::encode(val.write_to_bytes().unwrap()))
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
        // TODO: implement according to the logic of Zilliqa.
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
        let chain_id: u64 = self.query_jsonrpc_u64("CHAINID");
        (chain_id + BASE_CHAIN_ID).into()
    }

    fn exists(&self, address: H160) -> bool {
        // Try to query account balance, and see if it returns Some result.
        self.query_state_value(address, "_balance", None, true)
            .expect("query_state_value _balance")
            .is_some()
    }

    fn basic(&self, address: H160) -> Basic {
        let result = self
            .query_state_value(address, "_balance", None, true)
            .expect("query_state_value _balance")
            .map(|x| x.as_u64().unwrap_or_default())
            .unwrap_or_default();
        let balance = U256::from(result);
        let result = self
            .query_state_value(address, "_nonce", None, false)
            .expect("query_state_value _nonce")
            .map(|x| x.as_u64().unwrap_or_default())
            .unwrap_or_default();
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
        let result = self
            .query_state_value(address, "_evm_storage", Some(key), true)
            .expect("query_state_value(_evm_storage)")
            .unwrap_or_default();
        let mut result = hex::decode(result.as_str().unwrap_or_default()).unwrap_or_default();
        // H256::from_slice expects big-endian, we filled the first bytes from decoding,
        // now need to extend to the required size.
        result.resize(256 / 8, 0u8);
        H256::from_slice(&result)
    }

    // We implement original_storage via storage, as we postpone writes until
    // contract commit time.
    fn original_storage(&self, address: H160, key: H256) -> Option<H256> {
        Some(self.storage(address, key))
    }
}
