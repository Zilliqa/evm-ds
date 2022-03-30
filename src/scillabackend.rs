/// Backend implementation that stores EVM state in Scilla variables.
use evm::backend::{Backend, Basic};
use primitive_types::{H160, H256, U256};

pub struct ScillaBackend;

// impl<'config> StackState<'config> for ScillaState {
//     fn metadata(&self) -> &StackSubstateMetadata<'config>;
//     fn metadata_mut(&mut self) -> &mut StackSubstateMetadata<'config>;

//     fn enter(&mut self, gas_limit: u64, is_static: bool) {
//         // TODO
//     }

//     fn exit_commit(&mut self) -> Result<(), ExitError> {
//         // TODO
//         Ok(())
//     }

//     fn exit_revert(&mut self) -> Result<(), ExitError> {
//         // TODO
//         Ok(())
//     }

//     fn exit_discard(&mut self) -> Result<(), ExitError> {
//         // TODO
//         Ok(())
//     }

//     fn is_empty(&self, address: H160) -> bool {
//         // TODO
//         false
//     }

//     fn deleted(&self, address: H160) -> bool {
//         // TODO
//         false
//     }

//     fn is_cold(&self, address: H160) -> bool {
//         // TODO
//         false
//     }

//     fn is_storage_cold(&self, address: H160, key: H256) -> bool {
//         // TODO
//         false
//     }

//     fn inc_nonce(&mut self, address: H160) {
//         // TODO
//     }

//     fn set_storage(&mut self, address: H160, key: H256, value: H256) {
//         // TODO
//     }

//     fn reset_storage(&mut self, address: H160) {
//         // TODO
//     }

//     fn log(&mut self, address: H160, topics: Vec<H256>, data: Vec<u8>) {
//         // TODO
//     }

//     fn set_deleted(&mut self, address: H160) {
//         // TODO
//     }

//     fn set_code(&mut self, address: H160, code: Vec<u8>) {
//         // TODO
//     }

//     fn transfer(&mut self, transfer: Transfer) -> Result<(), ExitError> {
//         Ok(())
//     }

//     fn reset_balance(&mut self, address: H160) {
//         // TODO
//     }

//     fn touch(&mut self, address: H160) {
//         // TODO
//     }
// }

impl<'config> Backend for ScillaBackend {
    fn gas_price(&self) -> U256 {
        U256::zero()
        // self.backend.gas_price()
    }
    fn origin(&self) -> H160 {
        H160::zero()
        // self.backend.origin()
    }
    fn block_hash(&self, number: U256) -> H256 {
        H256::zero()
        // self.backend.block_hash(number)
    }
    fn block_number(&self) -> U256 {
        U256::zero()
        // self.backend.block_number()
    }
    fn block_coinbase(&self) -> H160 {
        H160::zero()
        // self.backend.block_coinbase()
    }
    fn block_timestamp(&self) -> U256 {
        U256::zero()
        // self.backend.block_timestamp()
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
        U256::from(123)
        // self.backend.chain_id()
    }

    fn exists(&self, address: H160) -> bool {
        // self.substate.known_account(address).is_some() || self.backend.exists(address)
        false
    }

    fn basic(&self, address: H160) -> Basic {
        // self.substate
        //     .known_basic(address)
        //     .unwrap_or_else(|| self.backend.basic(address))
        Basic {
            balance: U256::zero(),
            nonce: U256::zero(),
        }
    }

    fn code(&self, address: H160) -> Vec<u8> {
        vec![0, 1, 2, 3, 4]
        // self.substate
        //     .known_code(address)
        //     .unwrap_or_else(|| self.backend.code(address))
    }

    fn storage(&self, address: H160, key: H256) -> H256 {
        H256::zero()
        // self.substate
        //     .known_storage(address, key)
        //     .unwrap_or_else(|| self.backend.storage(address, key))
    }

    fn original_storage(&self, address: H160, key: H256) -> Option<H256> {
        Some(H256::zero())
        // if let Some(value) = self.substate.known_original_storage(address, key) {
        //     return Some(value);
        // }

        // self.backend.original_storage(address, key)
    }
}
