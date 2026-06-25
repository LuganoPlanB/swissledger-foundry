use alloy_network::Network;
use alloy_primitives::{Address, TxHash};
use eyre::Result;
use std::marker::PhantomData;

/// Browser signer stub for the SwissLedger slim wallet crate.
#[derive(Clone, Debug)]
pub struct BrowserSigner<N: Network> {
    address: Address,
    _network: PhantomData<N>,
}

impl<N: Network> BrowserSigner<N> {
    pub const fn new(address: Address) -> Self {
        Self { address, _network: PhantomData }
    }

    pub const fn address(&self) -> Address {
        self.address
    }

    pub async fn send_transaction_via_browser(&self, _tx: N::TransactionRequest) -> Result<TxHash> {
        eyre::bail!("browser wallets are not supported in the SwissLedger slim build")
    }
}
