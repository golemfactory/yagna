use std::collections::HashSet;
use std::ops::Not;
use std::string::ToString;

use chrono::{DateTime, Utc};
use semver::Version;
use serde::Serializer;
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
pub const DEMAND_MANIFEST_NODE_DESCRIPTOR_PROPERTY: &str = "golem.!exp.gap-31.v0.node.descriptor";

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

/// # Computation Payload Manifest
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppManifest {
    #[cfg_attr(feature = "schema", schemars(with = "String", description = "Semver"))]
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

    pub fn get_outbound_access(&self) -> Option<OutboundAccess> {
        self.comp_manifest
            .as_ref()
            .and_then(|comp| comp.net.as_ref())
            .and_then(|net| net.inet.as_ref())
            .and_then(|inet| inet.out.as_ref())
            .map(|out| out.access.clone())
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

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "Application Metadata"))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMetadata {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<Version>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

/// # Payload
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// # Payload Platform
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadPlatform {
    pub arch: String,
    pub os: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
}

/// # Computation Manifest
/// Computation Manifests lets Requestors to define a certain set of allowed actions,
/// to be negotiated with and approved by a Provider.
/// Requestors' actions will be verified against the Manifest during computation.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompManifest {
    /// # Version
    /// Specifies a version (Semantic Versioning 2.0 specification) of the manifest.
    #[cfg_attr(feature = "schema", schemars(with = "String", description = "Semver"))]
    pub version: Version,
    /// # Script
    /// Defines a set of allowed ExeScript commands and applies constraints to their arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
    /// # Net
    /// Applies constraints to networking. Currently, outgoing requests to the public Internet network are covered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<Net>,
}

/// # Script
/// Defines a set of allowed ExeScript commands and applies constraints to their arguments.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    /// Specifies a curated list of commands.
    pub commands: Vec<Command>,
    /// Selects a default way of comparing command arguments stated in the manifest
    /// and the ones received in the ExeScript,
    /// unless stated otherwise in a command JSON object.
    #[serde(rename = "match", default)]
    pub arg_match: ArgMatch,
}

/// # Command
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Command {
    /// UTF-8 encoded string.
    /// No command context or matching mode need to be specified.
    /// E.g. ["run /bin/cat /etc/motd", "run /bin/date -R"]
    String(String),
    /// UTF-8 encoded JSON string.
    /// Command context (e.g. env) or argument matching mode need to be specified for a command.
    /// E.g. ["{\"run\": { \"args\": \"/bin/date -R\", \"env\": { \"MYVAR\": \"42\", \"match\": \"strict\" }}}"]
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

/// # Argument Match
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, EnumString, AsRefStr)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub enum ArgMatch {
    /// Byte-to-byte argument equality (default).
    #[strum(ascii_case_insensitive)]
    #[default]
    Strict,
    /// Treat argument as regular expression.
    /// Syntax: Perl-compatible regular expressions (UTF-8 Unicode mode),
    /// w/o the support for look around and backreferences (among others).
    #[strum(ascii_case_insensitive)]
    Regex,
}

/// # Net
/// Applies constraints to networking.
/// Currently, outgoing requests to the public Internet network are covered.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Net {
    /// # Internet Network
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inet: Option<Inet>,
}

/// # Internet Network
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inet {
    /// Internet Outbound Network
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<InetOut>,
}

/// # Internet Outbound Network
/// Applies constraints to networking.
/// Currently, outgoing requests to the public Internet network are covered.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InetOut {
    /// List of allowed outbound protocols.
    /// Supports "http", "https", "ws", and "wss".
    #[serde(default = "default_protocols")]
    pub protocols: Vec<String>,
    /// Outbound access
    #[serde(flatten)]
    #[cfg_attr(feature = "schema", schemars(with = "outbound_access::Representation"))]
    pub access: OutboundAccess,
}

#[derive(PartialEq, Clone, Debug)]
pub enum OutboundAccess {
    Urls(Vec<Url>),
    Unrestricted,
}

impl OutboundAccess {
    pub fn is_outbound_requested(&self) -> bool {
        match self {
            OutboundAccess::Urls(urls) => urls.is_empty().not(),
            OutboundAccess::Unrestricted => true,
        }
    }
}

mod outbound_access {
    use super::*;

    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", rename = "OutboundAccess", untagged)] //Untagged is used here to bypass "schemars" adding "additionalProperties: false" when using enum
    pub enum Representation {
        /// List of allowed external URLs that outbound requests can be sent to.
        /// Empty list means no outbound access is requested.
        /// E.g. ["http://golemfactory.s3.amazonaws.com/file1", "http://golemfactory.s3.amazonaws.com/file2"]
        Urls { urls: Vec<Url> },
        /// Every URL is allowed for outbound connection.
        Unrestricted { unrestricted: Unrestricted },
    }

    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Unrestricted {
        /// Only "true" value is valid in "unrestricted" case.
        urls: bool,
    }

    impl Serialize for OutboundAccess {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let repr = {
                match self {
                    OutboundAccess::Urls(urls) => Representation::Urls {
                        urls: urls.to_vec(),
                    },
                    OutboundAccess::Unrestricted => Representation::Unrestricted {
                        unrestricted: Unrestricted { urls: true },
                    },
                }
            };
            repr.serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for OutboundAccess {
        fn deserialize<D>(deserializer: D) -> Result<OutboundAccess, D::Error>
        where
            D: Deserializer<'de>,
        {
            let repr = Representation::deserialize(deserializer)?;

            match repr {
                Representation::Urls { urls } => Ok(OutboundAccess::Urls(urls)),
                Representation::Unrestricted { unrestricted } => {
                    if unrestricted.urls {
                        Ok(OutboundAccess::Unrestricted)
                    } else {
                        Err(serde::de::Error::custom(
                            "'unrestricted.urls: false' is not valid",
                        ))
                    }
                }
            }
        }
    }
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
    use base64::{engine::general_purpose, Engine as _};
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
                            access: OutboundAccess::Urls(vec![]),
                        }),
                    }),
                }),
            }),
        };

        let serialized = serde_json::to_string(&manifest).unwrap();

        println!("{}", serialized);
        println!("{}", general_purpose::STANDARD.encode(serialized));
    }

    mod outbound_access_serde {
        use super::*;

        #[test]
        fn access_is_none() {
            let json = serde_json::json!({ "protocols": default_protocols() });
            assert!(serde_json::from_value::<InetOut>(json).is_err());
        }

        #[test]
        fn access_is_urls() {
            let json = serde_json::json!({
                "protocols": default_protocols(),
                "urls": [ "https://example.net/" ]
            });
            let inet_out = InetOut {
                protocols: default_protocols(),
                access: OutboundAccess::Urls([Url::parse("https://example.net/").unwrap()].into()),
            };

            assert_eq!(serde_json::to_value(&inet_out).unwrap(), json);
            assert_eq!(serde_json::from_value::<InetOut>(json).unwrap(), inet_out);
        }

        #[test]
        fn access_is_unrestricted() {
            let json = serde_json::json!({
                "protocols": default_protocols(),
                "unrestricted": { "urls": true }
            });
            let inet_out = InetOut {
                protocols: default_protocols(),
                access: OutboundAccess::Unrestricted,
            };

            assert_eq!(serde_json::to_value(&inet_out).unwrap(), json);
            assert_eq!(serde_json::from_value::<InetOut>(json).unwrap(), inet_out);
        }

        #[test]
        fn json_access_has_invalid_value() {
            let json = serde_json::json!({
                "protocols": default_protocols(),
                "unrestricted": { "urls": false }
            });

            assert!(serde_json::from_value::<InetOut>(json).is_err());
        }
    }
}
