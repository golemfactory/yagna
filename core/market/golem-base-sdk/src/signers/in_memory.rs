use alloy::primitives::Address;
use alloy::signers::k256::ecdsa::{SigningKey, VerifyingKey};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::SignerSync;
use anyhow::Context;
use async_trait::async_trait;
use rand::thread_rng;
use std::fs;
use std::path::PathBuf;

use crate::account::TransactionSigner;

const DEFAULT_KEYSTORE_DIR: &str = ".local/share/GolemBase";

/// A signer that keeps the private key in memory
pub struct InMemorySigner {
    signer: PrivateKeySigner,
}

impl InMemorySigner {
    /// Gets the default keystore directory path
    fn get_keystore_dir() -> anyhow::Result<PathBuf> {
        let path = dirs::home_dir()
            .context("Could not find home directory")?
            .join(DEFAULT_KEYSTORE_DIR);

        // Create directory only if it doesn't exist
        if !path.exists() {
            fs::create_dir_all(&path)?;
        }
        Ok(path)
    }

    /// Generates a new random private key
    pub fn generate() -> Self {
        let signer = PrivateKeySigner::random();
        Self { signer }
    }

    /// Returns the private key
    pub fn private_key(&self) -> SigningKey {
        self.signer.credential().clone()
    }

    /// Returns the public key
    pub fn public_key(&self) -> VerifyingKey {
        self.signer.credential().verifying_key().clone()
    }

    /// Saves the private key to a file in the standard directory using keystore format
    pub fn save(&self, password: &str) -> anyhow::Result<PathBuf> {
        let path = Self::get_keystore_dir()?;
        let name = format!("key_{}.json", self.address());

        let mut rng = thread_rng();
        PrivateKeySigner::encrypt_keystore(
            &path,
            &mut rng,
            self.signer.credential().to_bytes(),
            password,
            Some(&name),
        )?;

        Ok(path)
    }

    /// Loads a private key from a keystore file
    pub fn load(path: PathBuf, password: &str) -> anyhow::Result<Self> {
        let signer = PrivateKeySigner::decrypt_keystore(&path, password)?;
        Ok(Self { signer })
    }

    /// Loads a signer by address from the default directory
    pub fn load_by_address(address: Address, password: &str) -> anyhow::Result<Self> {
        let path = Self::get_keystore_dir()?.join(format!("key_{}.json", address));
        Self::load(path, password)
    }

    /// Lists all local accounts in the keystore directory
    pub fn list_local_accounts() -> anyhow::Result<Vec<Address>> {
        let keystore_dir = Self::get_keystore_dir()?;
        let mut accounts = Vec::new();

        if let Ok(entries) = std::fs::read_dir(keystore_dir) {
            for entry in entries.flatten() {
                if let Some(file_name) = entry.file_name().to_str() {
                    if let Some(address) = Self::parse_keystore_filename(file_name) {
                        accounts.push(address);
                    }
                }
            }
        }

        Ok(accounts)
    }

    /// Parses an address from a keystore filename
    fn parse_keystore_filename(file_name: &str) -> Option<Address> {
        if !file_name.starts_with("key_") || !file_name.ends_with(".json") {
            return None;
        }

        file_name
            .strip_prefix("key_")
            .and_then(|s| s.strip_suffix(".json"))
            .and_then(|address_str| Address::parse_checksummed(address_str, None).ok())
    }
}

#[async_trait]
impl TransactionSigner for InMemorySigner {
    fn address(&self) -> Address {
        self.signer.address()
    }

    async fn sign(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        Ok(self.signer.sign_message_sync(data)?.as_bytes().to_vec())
    }
}
