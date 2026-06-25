//! Tempo predeployed contracts and bindings (SwissLedger stub).

#![no_std]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

use alloy_primitives::{Address, B256, address, b256};

pub const MULTICALL3_ADDRESS: Address = address!("0xcA11bde05977b3631167028862bE2a173976CA11");
pub const CREATEX_ADDRESS: Address = address!("0xba5Ed099633D3B313e4D5F7bdc1305d3c28ba5Ed");
pub const SAFE_DEPLOYER_ADDRESS: Address = address!("0x914d7Fec6aaC8cd542e72Bca78B30650d45643d7");
pub const PERMIT2_ADDRESS: Address = address!("0x000000000022d473030f116ddee9f6b43ac78ba3");
pub const PERMIT2_SALT: B256 =
    b256!("0x0000000000000000000000000000000000000000d3af2663da51c10215000000");
pub const ARACHNID_CREATE2_FACTORY_ADDRESS: Address =
    address!("0x4e59b44847b379578588920cA78FbF26c0B4956C");

/// Stub binding for the Multicall3 deployment bytecode.
pub struct Multicall3;

impl Multicall3 {
    pub const DEPLOYED_BYTECODE: &'static [u8] = &[];
}

/// Stub binding for the CreateX deployment bytecode.
pub struct CreateX;

impl CreateX {
    pub const DEPLOYED_BYTECODE: &'static [u8] = &[];
}

/// Stub binding for the SafeDeployer deployment bytecode.
pub struct SafeDeployer;

impl SafeDeployer {
    pub const DEPLOYED_BYTECODE: &'static [u8] = &[];
}

/// Stub binding for the Permit2 deployment bytecode.
pub struct Permit2;

impl Permit2 {
    pub const DEPLOYED_BYTECODE: &'static [u8] = &[];
}

macro_rules! sol {
    ($($input:tt)*) => {
        #[cfg(all(feature = "rpc", feature = "serde"))]
        alloy_sol_types::sol! {
            #[sol(rpc)]
            #[derive(serde::Serialize, serde::Deserialize)]
            $($input)*
        }
        #[cfg(all(feature = "rpc", not(feature = "serde")))]
        alloy_sol_types::sol! {
            #[sol(rpc)]
            $($input)*
        }
        #[cfg(all(not(feature = "rpc"), feature = "serde"))]
        alloy_sol_types::sol! {
            #[derive(serde::Serialize, serde::Deserialize)]
            $($input)*
        }
        #[cfg(all(not(feature = "rpc"), not(feature = "serde")))]
        alloy_sol_types::sol! {
            $($input)*
        }
    };
}

pub(crate) use sol;

pub mod contracts {
    use alloy_primitives::{B256, Bytes, b256, bytes};

    /// Keccak256 hash of CreateX deployed bytecode
    pub const CREATEX_BYTECODE_HASH: B256 =
        b256!("0xbd8a7ea8cfca7b4e5f5041d7d4b17bc317c5ce42cfbc42066a00cf26b43eb53f");

    // Hardcode the CreateX, Permit2, SafeDeployer, Multicall3 bytecodes
    // instead of loading ABI JSONs (which requires sol! JSON support not in 1.5.7)
    pub const ARACHNID_CREATE2_FACTORY_BYTECODE: Bytes = bytes!("");
    pub const MULTICALL3_DEPLOYED_BYTECODE_HASH: B256 =
        b256!("0x0000000000000000000000000000000000000000000000000000000000000000");
}

pub mod precompiles;
