use std::env;

use ethsign::*;
use rustc_hex::FromHex;

use ya_core_model::NodeId;

use crate::id_key::IdentityKey;
use anyhow::Context;

const ENV_AUTOCONF_APP_KEY: &str = "YAGNA_AUTOCONF_APPKEY";

pub fn identity_from_env(
    password: Protected,
    env_name: &str,
) -> anyhow::Result<Option<IdentityKey>> {
    let secret_hex: Vec<u8> = match env::var(env_name) {
        Ok(v) => v
            .from_hex()
            .with_context(|| format!("Failed to parse identity from {}", env_name))?,
        Err(_) => return Ok(None),
    };
    let secret = SecretKey::from_raw(&secret_hex)?;
    Ok(Some(IdentityKey::from_secret(None, secret, password)))
}

pub fn preconfigured_node_id(env_name: &str) -> anyhow::Result<Option<NodeId>> {
    let secret_hex: Vec<u8> = match env::var(env_name) {
        Ok(v) => v.from_hex()?,
        Err(_) => return Ok(None),
    };
    let secret = SecretKey::from_raw(&secret_hex)?;
    Ok(Some(NodeId::from(secret.public().address().as_ref())))
}

pub fn preconfigured_appkey() -> Option<String> {
    env::var(ENV_AUTOCONF_APP_KEY).ok()
}
