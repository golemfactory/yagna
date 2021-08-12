#![allow(unused)]

use std::convert::TryFrom;

use anyhow::Context;
use ethsign::keyfile::Bytes;
use ethsign::{KeyFile, Protected, SecretKey};
use rand::Rng;
use ya_client_model::NodeId;

use crate::dao::identity::Identity;
use crate::dao::Error;

pub struct IdentityKey {
    id: NodeId,
    alias: Option<String>,
    key_file: KeyFile,
    secret: Option<SecretKey>,
}

impl IdentityKey {
    #[inline]
    pub fn id(&self) -> NodeId {
        self.id
    }

    #[inline]
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_ref().map(|s| s.as_ref())
    }

    pub fn replace_alias(&mut self, new_alias: Option<String>) -> Option<String> {
        std::mem::replace(&mut self.alias, new_alias)
    }

    pub fn to_key_file(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.key_file)
    }

    pub fn is_locked(&self) -> bool {
        self.secret.is_none()
    }

    pub fn unlock(&mut self, password: Protected) -> Result<bool, Error> {
        let secret = match self.key_file.to_secret_key(&password) {
            Ok(secret) => secret,
            Err(ethsign::Error::InvalidPassword) => return Ok(false),
            Err(e) => return Err(Error::internal(e)),
        };
        self.secret = Some(secret);
        Ok(true)
    }

    /// Sign given 32-byte message with the key.
    pub fn sign(&self, data: &[u8]) -> Option<Vec<u8>> {
        let s = match &self.secret {
            Some(secret) => secret,
            None => return None,
        };
        s.sign(data).ok().map(|s| {
            let mut v = Vec::with_capacity(33);

            v.push(s.v);
            v.extend_from_slice(&s.r[..]);
            v.extend_from_slice(&s.s[..]);

            v
        })
    }

    pub fn lock(&mut self, new_password: Option<String>) -> anyhow::Result<()> {
        if let Some(new_password) = new_password {
            if let Some(secret) = self.secret.take() {
                let crypto = secret.to_crypto(&Protected::new(new_password), KEY_ITERATIONS)?;
                self.key_file.crypto = crypto;
            } else {
                anyhow::bail!("key already locked")
            }
        } else {
            self.secret = None;
        }
        Ok(())
    }

    pub fn from_secret(alias: Option<String>, secret: SecretKey, password: Protected) -> Self {
        let key_file = key_file_from_secret(&secret, password);
        let id = NodeId::from(secret.public().address().as_ref());
        IdentityKey {
            id,
            alias,
            key_file,
            secret: Some(secret),
        }
    }
}

impl TryFrom<Identity> for IdentityKey {
    type Error = serde_json::Error;

    fn try_from(value: Identity) -> Result<Self, Self::Error> {
        let key_file: KeyFile = serde_json::from_str(&value.key_file_json)?;
        let id = value.identity_id;
        let alias = value.alias;
        let secret = key_file.to_secret_key(&Protected::new("")).ok();
        Ok(IdentityKey {
            id,
            alias,
            key_file,
            secret,
        })
    }
}

const KEY_ITERATIONS: u32 = 10240;
const KEYSTORE_VERSION: u64 = 3;

pub fn default_password() -> Protected {
    Protected::new(Vec::default())
}

pub fn generate_new(alias: Option<String>, password: Protected) -> IdentityKey {
    let (key_file, secret) = generate_new_secret(password);
    let id = NodeId::from(secret.public().address().as_ref());
    IdentityKey {
        id,
        alias,
        key_file,
        secret: Some(secret),
    }
}

fn generate_new_secret(password: Protected) -> (KeyFile, SecretKey) {
    let random_bytes: [u8; 32] = rand::thread_rng().gen();
    let secret = SecretKey::from_raw(random_bytes.as_ref()).unwrap();
    let key_file = key_file_from_secret(&secret, password);
    (key_file, secret)
}

fn key_file_from_secret(secret: &SecretKey, password: Protected) -> KeyFile {
    KeyFile {
        id: format!("{}", uuid::Uuid::new_v4()),
        version: KEYSTORE_VERSION,
        crypto: secret.to_crypto(&password, KEY_ITERATIONS).unwrap(),
        address: Some(Bytes(secret.public().address().to_vec())),
    }
}

pub fn generate_new_keyfile(password: Protected) -> anyhow::Result<String> {
    let (key_file, _) = generate_new_secret(password);

    Ok(serde_json::to_string(&key_file).context("serialize keyfile")?)
}

#[cfg(test)]
mod test {
    use rustc_hex::FromHex;

    use super::*;

    #[test]
    fn test_import_raw_key() -> anyhow::Result<()> {
        let pk = "c19a9a827c9efb910e3e4efb955b57d072775c5ebb93dbdd4d6856d97e555eca";
        let pk_bytes: Vec<u8> = pk.from_hex()?;
        println!("{}", pk.len());
        let secret: SecretKey = SecretKey::from_raw(&pk_bytes)?;
        let key_file = key_file_from_secret(&secret, Protected::new(""));
        println!("{}", serde_json::to_string_pretty(&key_file)?);
        Ok(())
    }
}
