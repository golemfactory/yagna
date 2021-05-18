use crate::dao::identity::Identity;
use crate::dao::Error;
use anyhow::Context;
use ethsign::keyfile::Bytes;
use ethsign::{KeyFile, Protected, SecretKey};
use rand::Rng;
use std::convert::TryFrom;
use ya_client_model::NodeId;

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

    pub fn lock(&mut self) {
        self.secret = None;
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
    let key_file = KeyFile {
        id: format!("{}", uuid::Uuid::new_v4()),
        version: KEYSTORE_VERSION,
        crypto: secret.to_crypto(&password, KEY_ITERATIONS).unwrap(),
        address: Some(Bytes(secret.public().address().to_vec())),
    };
    (key_file, secret)
}

pub fn generate_new_keyfile(password: Protected) -> anyhow::Result<String> {
    let (key_file, _) = generate_new_secret(password);

    Ok(serde_json::to_string(&key_file).context("serialize keyfile")?)
}
