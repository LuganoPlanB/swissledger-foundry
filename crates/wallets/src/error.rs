use alloy_primitives::hex::FromHexError;
use alloy_signer::k256::ecdsa;
use alloy_signer_local::LocalSignerError;

#[cfg(feature = "ledger")]
use alloy_signer_ledger::LedgerError;

#[cfg(feature = "trezor")]
use alloy_signer_trezor::TrezorError;

#[cfg(feature = "aws-kms")]
use alloy_signer_aws::AwsSignerError;

#[cfg(feature = "gcp-kms")]
use alloy_signer_gcp::GcpSignerError;

#[cfg(feature = "turnkey")]
use alloy_signer_turnkey::TurnkeySignerError;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Store error: {0}")]
    Internal(String),
}

#[derive(Debug, thiserror::Error)]
pub enum PrivateKeyError {
    #[error("Failed to create wallet from private key. Private key is invalid hex: {0}")]
    InvalidHex(#[from] FromHexError),
    #[error(
        "Failed to create wallet from private key. Invalid private key. But env var {0} exists. Is the `$` anchor missing?"
    )]
    ExistsAsEnvVar(String),
}

#[derive(Debug, thiserror::Error)]
pub enum WalletSignerError {
    #[error(transparent)]
    Local(#[from] LocalSignerError),
    #[error("Failed to decrypt keystore: incorrect password")]
    IncorrectKeystorePassword,
    #[cfg(feature = "ledger")]
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[cfg(feature = "trezor")]
    #[error(transparent)]
    Trezor(#[from] TrezorError),
    #[cfg(feature = "aws-kms")]
    #[error(transparent)]
    Aws(#[from] Box<AwsSignerError>),
    #[cfg(feature = "gcp-kms")]
    #[error(transparent)]
    Gcp(#[from] Box<GcpSignerError>),
    #[cfg(feature = "turnkey")]
    #[error(transparent)]
    Turnkey(#[from] TurnkeySignerError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    InvalidHex(#[from] FromHexError),
    #[error(transparent)]
    Ecdsa(#[from] ecdsa::Error),
    #[error("foundry was not built with support for {0} signer")]
    UnsupportedSigner(&'static str),
}

impl WalletSignerError {
    pub const fn aws_unsupported() -> Self {
        Self::UnsupportedSigner("AWS KMS")
    }
    pub const fn gcp_unsupported() -> Self {
        Self::UnsupportedSigner("Google Cloud KMS")
    }
    pub const fn turnkey_unsupported() -> Self {
        Self::UnsupportedSigner("Turnkey")
    }
}
