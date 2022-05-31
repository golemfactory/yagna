use std::io::Cursor;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeployResult {
    pub valid: Result<String, String>,
    #[serde(default)]
    pub vols: Vec<ContainerVolume>,
    #[serde(default)]
    pub start_mode: StartMode,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContainerVolume {
    pub name: String,
    pub path: String,
}

#[non_exhaustive]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ContainerEndpoint {
    Socket(PathBuf),
}

impl From<ContainerEndpoint> for PathBuf {
    fn from(e: ContainerEndpoint) -> Self {
        match e {
            ContainerEndpoint::Socket(p) => p,
        }
    }
}

impl From<crate::server::NetworkEndpoint> for ContainerEndpoint {
    fn from(endpoint: crate::server::NetworkEndpoint) -> Self {
        match endpoint {
            crate::server::NetworkEndpoint::Socket(s) => Self::Socket(PathBuf::from(s)),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StartMode {
    Empty,
    Blocking,
}

impl Default for StartMode {
    fn default() -> Self {
        StartMode::Empty
    }
}

impl DeployResult {
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> anyhow::Result<DeployResult> {
        let b: &[u8] = bytes.as_ref();
        if b.is_empty() {
            log::warn!("empty descriptor");
            let vols = if cfg!(feature = "compat-deployment") {
                vec![ContainerVolume {
                    name: ".".to_string(),
                    path: "".to_string(),
                }]
            } else {
                Default::default()
            };

            return Ok(DeployResult {
                valid: Ok(Default::default()),
                vols,
                start_mode: Default::default(),
            });
        }
        if let Some(idx) = b.iter().position(|&ch| ch == b'{') {
            let b = &b[idx..];
            Ok(serde_json::from_reader(Cursor::new(b))?)
        } else {
            let text = String::from_utf8_lossy(b);
            anyhow::bail!("invalid deploy response: {}", text);
        }
    }
}

#[cfg(test)]
mod test {
    use super::DeployResult;

    fn parse_bytes<T: AsRef<[u8]>>(b: T) -> DeployResult {
        let result = DeployResult::from_bytes(b).unwrap();
        eprintln!("result={:?}", result);
        result
    }

    #[test]
    fn test_wasi_deploy() {
        parse_bytes(
            r#"{
            "valid": {"Ok": "success"}
        }"#,
        );
        parse_bytes(
            r#"{
            "valid": {"Err": "bad image format"}
        }"#,
        );
        parse_bytes(
            r#"{
            "valid": {"Ok": "success"},
            "vols": [
                {"name": "vol-9a0c1c4a", "path": "/in"},
                {"name": "vol-a68672e0", "path": "/out"}
            ]
        }"#,
        );
    }
}
