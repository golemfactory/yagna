use std::convert::TryInto;
use std::env;

use ethsign::*;
use rustc_hex::FromHex;

use ya_core_model::NodeId;

use crate::id_key::IdentityKey;
use anyhow::Context;

// autoconfiguration
const ENV_AUTOCONF_PK: &str = "YAGNA_AUTOCONF_ID_SECRET";
const ENV_AUTOCONF_APP_KEY: &str = "YAGNA_AUTOCONF_APPKEY";

pub fn preconfigured_identity(password: Protected) -> anyhow::Result<Option<IdentityKey>> {
    let secret_raw: [u8; 32] = match env::var(ENV_AUTOCONF_PK) {
        Ok(v) => v
            .from_hex::<Vec<u8>>()
            .with_context(|| format!("Failed to parse identity from {}", ENV_AUTOCONF_PK))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Wrong length {}", ENV_AUTOCONF_PK))?,
        Err(_) => return Ok(None),
    };
    Ok(Some(IdentityKey::from_secret(None, secret_raw, password)?))
}

pub fn preconfigured_node_id() -> anyhow::Result<Option<NodeId>> {
    let secret_hex: Vec<u8> = match env::var(ENV_AUTOCONF_PK) {
        Ok(v) => v.from_hex()?,
        Err(_) => return Ok(None),
    };
    let secret = SecretKey::from_raw(&secret_hex)?;
    Ok(Some(NodeId::from(secret.public().address().as_ref())))
}

pub fn preconfigured_appkey() -> Option<String> {
    env::var(ENV_AUTOCONF_APP_KEY).ok()
}
