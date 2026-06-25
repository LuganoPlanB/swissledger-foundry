use crate::error::WalletSignerError;
use alloy_consensus::SignableTransaction;
use alloy_dyn_abi::TypedData;
use alloy_network::TxSigner;
use alloy_primitives::{Address, B256, ChainId, Signature, hex};
use alloy_signer::Signer;
use alloy_signer_local::{MnemonicBuilder, PrivateKeySigner, coins_bip39::English};
use alloy_sol_types::{Eip712Domain, SolStruct};
use async_trait::async_trait;
use std::{collections::HashSet, path::PathBuf};

#[cfg(feature = "ledger")]
use alloy_signer_ledger::{HDPath as LedgerHDPath, LedgerSigner};

#[cfg(feature = "trezor")]
use alloy_signer_trezor::{HDPath as TrezorHDPath, TrezorSigner};

#[cfg(feature = "aws-kms")]
use alloy_signer_aws::{AwsSigner, aws_config::BehaviorVersion, aws_sdk_kms::Client as AwsClient};

#[cfg(feature = "gcp-kms")]
use alloy_signer_gcp::{
    GcpKeyRingRef, GcpSigner, GcpSignerError, KeySpecifier,
    gcloud_sdk::{
        GoogleApi,
        google::cloud::kms::v1::key_management_service_client::KeyManagementServiceClient,
    },
};

#[cfg(feature = "turnkey")]
use alloy_signer_turnkey::TurnkeySigner;

pub type Result<T> = std::result::Result<T, WalletSignerError>;

#[derive(Debug)]
pub enum WalletSigner {
    Local(PrivateKeySigner),
    #[cfg(feature = "ledger")]
    Ledger(LedgerSigner),
    #[cfg(feature = "trezor")]
    Trezor(TrezorSigner),
    #[cfg(feature = "aws-kms")]
    Aws(AwsSigner),
    #[cfg(feature = "gcp-kms")]
    Gcp(GcpSigner),
    #[cfg(feature = "turnkey")]
    Turnkey(TurnkeySigner),
}

impl WalletSigner {
    #[cfg(feature = "ledger")]
    pub async fn from_ledger_path(path: LedgerHDPath) -> Result<Self> {
        let ledger = LedgerSigner::new(path, None).await?;
        Ok(Self::Ledger(ledger))
    }

    #[cfg(not(feature = "ledger"))]
    pub async fn from_ledger_path(_path: ()) -> Result<Self> {
        Err(WalletSignerError::UnsupportedSigner("Ledger"))
    }

    #[cfg(feature = "trezor")]
    pub async fn from_trezor_path(path: TrezorHDPath) -> Result<Self> {
        let trezor = TrezorSigner::new(path, None).await?;
        Ok(Self::Trezor(trezor))
    }

    #[cfg(not(feature = "trezor"))]
    pub async fn from_trezor_path(_path: ()) -> Result<Self> {
        Err(WalletSignerError::UnsupportedSigner("Trezor"))
    }

    pub async fn from_aws(key_id: String) -> Result<Self> {
        #[cfg(feature = "aws-kms")]
        {
            let config =
                alloy_signer_aws::aws_config::load_defaults(BehaviorVersion::latest()).await;
            let client = AwsClient::new(&config);
            Ok(Self::Aws(
                AwsSigner::new(client, key_id, None)
                    .await
                    .map_err(|e| WalletSignerError::Aws(Box::new(e)))?,
            ))
        }
        #[cfg(not(feature = "aws-kms"))]
        {
            let _ = key_id;
            Err(WalletSignerError::aws_unsupported())
        }
    }

    pub async fn from_gcp(
        project_id: String,
        location: String,
        keyring: String,
        key_name: String,
        key_version: u64,
    ) -> Result<Self> {
        #[cfg(feature = "gcp-kms")]
        {
            let keyring = GcpKeyRingRef::new(&project_id, &location, &keyring);
            let client = match GoogleApi::from_function(
                KeyManagementServiceClient::new,
                "https://cloudkms.googleapis.com",
                None,
            )
            .await
            {
                Ok(c) => c,
                Err(e) => {
                    return Err(WalletSignerError::Gcp(Box::new(GcpSignerError::GoogleKmsError(e))));
                }
            };
            let specifier = KeySpecifier::new(keyring, &key_name, key_version);
            Ok(Self::Gcp(
                GcpSigner::new(client, specifier, None)
                    .await
                    .map_err(|e| WalletSignerError::Gcp(Box::new(e)))?,
            ))
        }
        #[cfg(not(feature = "gcp-kms"))]
        {
            let _ = project_id;
            let _ = location;
            let _ = keyring;
            let _ = key_name;
            let _ = key_version;
            Err(WalletSignerError::gcp_unsupported())
        }
    }

    pub fn from_turnkey(
        api_private_key: String,
        organization_id: String,
        address: Address,
    ) -> Result<Self> {
        #[cfg(feature = "turnkey")]
        {
            Ok(Self::Turnkey(TurnkeySigner::from_api_key(
                &api_private_key,
                organization_id,
                address,
                None,
            )?))
        }
        #[cfg(not(feature = "turnkey"))]
        {
            let _ = api_private_key;
            let _ = organization_id;
            let _ = address;
            Err(WalletSignerError::turnkey_unsupported())
        }
    }

    pub fn from_private_key(private_key: &B256) -> Result<Self> {
        Ok(Self::Local(PrivateKeySigner::from_bytes(private_key)?))
    }

    pub async fn available_senders(&self, _max: usize) -> Result<Vec<Address>> {
        let mut senders = HashSet::new();
        match self {
            Self::Local(local) => {
                senders.insert(local.address());
            }
            #[cfg(feature = "ledger")]
            Self::Ledger(ledger) => {
                for i in 0..max {
                    match ledger.get_address_with_path(&LedgerHDPath::LedgerLive(i)).await {
                        Ok(address) => { senders.insert(address); }
                        Err(e) => { warn!("Failed to get Ledger address at index {i} (LedgerLive): {e}"); }
                    }
                }
                for i in 0..max {
                    match ledger.get_address_with_path(&LedgerHDPath::Legacy(i)).await {
                        Ok(address) => { senders.insert(address); }
                        Err(e) => { warn!("Failed to get Ledger address at index {i} (Legacy): {e}"); }
                    }
                }
            }
            #[cfg(feature = "trezor")]
            Self::Trezor(trezor) => {
                for i in 0..max {
                    match trezor.get_address_with_path(&TrezorHDPath::TrezorLive(i)).await {
                        Ok(address) => { senders.insert(address); }
                        Err(e) => { warn!("Failed to get Trezor address at index {i} (TrezorLive): {e}"); }
                    }
                }
            }
            #[cfg(feature = "aws-kms")]
            Self::Aws(aws) => { senders.insert(alloy_signer::Signer::address(aws)); }
            #[cfg(feature = "gcp-kms")]
            Self::Gcp(gcp) => { senders.insert(alloy_signer::Signer::address(gcp)); }
            #[cfg(feature = "turnkey")]
            Self::Turnkey(turnkey) => { senders.insert(alloy_signer::Signer::address(turnkey)); }
        }
        Ok(senders.into_iter().collect())
    }

    pub fn from_mnemonic(
        mnemonic: &str,
        passphrase: Option<&str>,
        derivation_path: Option<&str>,
        index: u32,
    ) -> Result<Self> {
        let mut builder = MnemonicBuilder::<English>::default().phrase(mnemonic);
        if let Some(passphrase) = passphrase {
            builder = builder.password(passphrase)
        }
        builder = if let Some(hd_path) = derivation_path {
            builder.derivation_path(hd_path)?
        } else {
            builder.index(index)?
        };
        Ok(Self::Local(builder.build()?))
    }
}

macro_rules! delegate {
    ($s:ident, $inner:ident => $e:expr) => {
        match $s {
            Self::Local($inner) => $e,
            #[cfg(feature = "ledger")]
            Self::Ledger($inner) => $e,
            #[cfg(feature = "trezor")]
            Self::Trezor($inner) => $e,
            #[cfg(feature = "aws-kms")]
            Self::Aws($inner) => $e,
            #[cfg(feature = "gcp-kms")]
            Self::Gcp($inner) => $e,
            #[cfg(feature = "turnkey")]
            Self::Turnkey($inner) => $e,
        }
    };
}

#[async_trait]
impl Signer for WalletSigner {
    async fn sign_hash(&self, hash: &B256) -> alloy_signer::Result<Signature> {
        delegate!(self, inner => inner.sign_hash(hash)).await
    }
    async fn sign_message(&self, message: &[u8]) -> alloy_signer::Result<Signature> {
        delegate!(self, inner => inner.sign_message(message)).await
    }
    fn address(&self) -> Address {
        delegate!(self, inner => alloy_signer::Signer::address(inner))
    }
    fn chain_id(&self) -> Option<ChainId> {
        delegate!(self, inner => inner.chain_id())
    }
    fn set_chain_id(&mut self, chain_id: Option<ChainId>) {
        delegate!(self, inner => inner.set_chain_id(chain_id))
    }
    async fn sign_typed_data<T: SolStruct + Send + Sync>(
        &self,
        payload: &T,
        domain: &Eip712Domain,
    ) -> alloy_signer::Result<Signature>
    where Self: Sized {
        delegate!(self, inner => inner.sign_typed_data(payload, domain)).await
    }
    async fn sign_dynamic_typed_data(
        &self,
        payload: &TypedData,
    ) -> alloy_signer::Result<Signature> {
        delegate!(self, inner => inner.sign_dynamic_typed_data(payload)).await
    }
}

#[async_trait]
impl TxSigner<Signature> for WalletSigner {
    fn address(&self) -> Address {
        Signer::address(self)
    }
    async fn sign_transaction(
        &self,
        tx: &mut dyn SignableTransaction<Signature>,
    ) -> alloy_signer::Result<Signature> {
        delegate!(self, inner => TxSigner::sign_transaction(inner, tx)).await
    }
}

#[derive(Debug, Clone)]
pub enum PendingSigner {
    Keystore(PathBuf),
    Interactive,
}

impl PendingSigner {
    pub fn unlock(self) -> Result<WalletSigner> {
        match self {
            Self::Keystore(path) => {
                let password = rpassword::prompt_password("Enter keystore password:")?;
                match PrivateKeySigner::decrypt_keystore(path, password) {
                    Ok(signer) => Ok(WalletSigner::Local(signer)),
                    Err(e) => match e {
                        alloy_signer_local::LocalSignerError::EthKeystoreError(
                            eth_keystore::KeystoreError::MacMismatch,
                        ) => Err(WalletSignerError::IncorrectKeystorePassword),
                        _ => Err(WalletSignerError::Local(e)),
                    },
                }
            }
            Self::Interactive => {
                let private_key = rpassword::prompt_password("Enter private key:")?;
                Ok(WalletSigner::from_private_key(&hex::FromHex::from_hex(private_key)?)?)
            }
        }
    }
}
