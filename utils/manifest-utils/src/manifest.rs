use std::collections::HashSet;
use std::ops::Not;
use std::string::ToString;

use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use strum;
use strum::AsRefStr;
use strum::Display;
use strum::EnumString;
use url::Url;

use ya_agreement_utils::AgreementView;
use ya_agreement_utils::Error as AgreementError;

use crate::decode_data;

pub const CAPABILITIES_PROPERTY: &str = "golem.runtime.capabilities";
pub const DEMAND_MANIFEST_PROPERTY: &str = "golem.srv.comp.payload";
pub const DEMAND_MANIFEST_SIG_PROPERTY: &str = "golem.srv.comp.payload.sig";
pub const DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY: &str = "golem.srv.comp.payload.sig.algorithm";
pub const DEMAND_MANIFEST_CERT_PROPERTY: &str = "golem.srv.comp.payload.cert";

pub const AGREEMENT_MANIFEST_PROPERTY: &str = "demand.properties.golem.srv.comp.payload";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("agreement error: {0}")]
    AgreementError(#[from] AgreementError),
    #[error(transparent)]
    DecodingError(#[from] crate::DecodingError),
    #[error("invalid input json encoding: {0}")]
    BlobJsonEncoding(#[from] snailquote::ParseUnicodeError),
    #[error("invalid hash format: '{0}'")]
    HashFormat(String),
    #[error("invalid hash: {0}")]
    HashHexValue(#[from] hex::FromHexError),
    #[error("invalid manifest format: {0}")]
    ManifestFormat(#[from] serde_json::Error),
    #[error("invalid signature format: {0}")]
    SignatureFormat(String),
    #[error("unsupported signature algorithm")]
    SignatureNotSupported,
}

pub fn read_manifest(view: &AgreementView) -> Result<Option<AppManifest>, Error> {
    let manifest: String = match view.get_property(AGREEMENT_MANIFEST_PROPERTY) {
        Ok(value) => value,
        Err(AgreementError::NoKey(_)) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    Ok(Some(decode_manifest(manifest)?))
}

pub fn decode_manifest<S: AsRef<str>>(data: S) -> Result<AppManifest, Error> {
    let data = decode_data(data)?;
    Ok(serde_json::de::from_slice(&data)?)
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum Feature {
    Inet,
    Vpn,
    ManifestSupport,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppManifest {
    pub version: Version,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AppMetadata>,
    pub payload: Vec<AppPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comp_manifest: Option<CompManifest>,
}

impl AppManifest {
    pub fn find_payload(&self, arch: &str, os: &str) -> Option<String> {
        // TODO: check OS version, if present
        self.payload
            .iter()
            .find(|payload| {
                let url_present = payload.urls.is_empty().not();
                let hash_present = payload.hash.is_empty().not();
                let platform_matches = match payload.platform {
                    Some(ref platform) => {
                        platform.arch.as_str() == arch && platform.os.as_str() == os
                    }
                    _ => true,
                };
                platform_matches && url_present && hash_present
            })
            .map(|payload| {
                let url = payload.urls.first().unwrap();
                format!("hash:{}:{}", payload.hash, url)
            })
    }

    pub fn features(&self) -> HashSet<Feature> {
        let mut features = HashSet::new();

        if let Some(ref comp) = self.comp_manifest {
            if comp
                .net
                .as_ref()
                .map(|net| net.inet.is_some())
                .unwrap_or(false)
            {
                features.insert(Feature::Inet);
            }
        }

        features
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMetadata {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<Version>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<PayloadPlatform>,
    pub urls: Vec<Url>,
    pub hash: String,
}

impl AppPayload {
    pub fn parse_hash(&self) -> Result<(String, Vec<u8>), Error> {
        let mut split = self.hash.splitn(2, ':');
        let algo = split
            .next()
            .ok_or_else(|| Error::HashFormat(self.hash.clone()))?
            .to_string();
        let bytes = hex::decode(
            split
                .next()
                .ok_or_else(|| Error::HashFormat(self.hash.clone()))?,
        )?;
        Ok((algo, bytes))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadPlatform {
    pub arch: String,
    pub os: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompManifest {
    pub version: Version,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<Net>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub commands: Vec<Command>,
    #[serde(rename = "match", default)]
    pub arg_match: ArgMatch,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Command {
    String(String),
    Json(Value),
}

struct CommandVisitor;

impl<'de> serde::de::Visitor<'de> for CommandVisitor {
    type Value = Command;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("command string or a json string with a command object")
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(match serde_json::from_str::<Value>(s) {
            Ok(inner) => Command::Json(inner),
            Err(_) => Command::String(s.to_string()),
        })
    }
}

impl<'de> Deserialize<'de> for Command {
    fn deserialize<D>(deserializer: D) -> Result<Command, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(CommandVisitor)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, EnumString, AsRefStr)]
#[serde(rename_all = "camelCase")]
pub enum ArgMatch {
    #[strum(ascii_case_insensitive)]
    Strict,
    #[strum(ascii_case_insensitive)]
    Regex,
}

impl Default for ArgMatch {
    fn default() -> Self {
        ArgMatch::Strict
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Net {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inet: Option<Inet>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<InetOut>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InetOut {
    #[serde(default = "default_protocols")]
    pub protocols: Vec<String>,
    // keep the option here to retain information on
    // whether urls were specified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urls: Option<Vec<Url>>,
}

pub fn default_protocols() -> Vec<String> {
    ["http", "https", "ws", "wss"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn serialize_manifest() {
        let url = Url::parse(
            "http://yacn2.dev.golem.network:8000/docker-chainlink-latest-13d419a227.gvmi",
        )
        .unwrap();

        let manifest = AppManifest {
            version: Version::new(0, 1, 0),
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::days(3650),
            metadata: Some(AppMetadata {
                name: "example manifest".to_string(),
                description: Some("example description".to_string()),
                version: Some(Version::new(0, 1, 0)),
                authors: Vec::new(),
                homepage: None,
            }),
            payload: vec![AppPayload {
                platform: Some(PayloadPlatform {
                    arch: std::env::consts::ARCH.to_string(),
                    os: std::env::consts::OS.to_string(),
                    os_version: None,
                }),
                urls: vec![url],
                hash: "sha3:55aa1909f03b57e25a2f11792ded100c430296335ed2ccf9554dcf9d".to_string(),
            }],
            comp_manifest: Some(CompManifest {
                version: Version::new(0, 1, 0),
                script: Some(Script {
                    commands: vec![
                        Command::String("run .*".to_string()),
                        Command::String("transfer .*".to_string()),
                    ],
                    arg_match: ArgMatch::Regex,
                }),
                net: Some(Net {
                    inet: Some(Inet {
                        out: Some(InetOut {
                            protocols: default_protocols(),
                            urls: None,
                        }),
                    }),
                }),
            }),
        };

        let serialized = serde_json::to_string(&manifest).unwrap();

        println!("{}", serialized);
        println!("{}", base64::encode(serialized));
    }
}
