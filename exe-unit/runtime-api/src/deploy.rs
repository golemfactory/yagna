use std::convert::TryFrom;
use std::io::Cursor;
use std::net::{AddrParseError, Ipv4Addr, SocketAddr};
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

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContainerVolume {
    pub name: String,
    pub path: String,
}

#[non_exhaustive]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ContainerEndpoint {
    UnixStream(PathBuf),
    UnixDatagram(PathBuf),
    UdpDatagram(SocketAddr),
    TcpListener(SocketAddr),
    TcpStream(SocketAddr),
}

impl std::fmt::Display for ContainerEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnixStream(path) | Self::UnixDatagram(path) => write!(f, "{}", path.display()),
            Self::UdpDatagram(addr) | Self::TcpListener(addr) | Self::TcpStream(addr) => {
                write!(f, "{}", addr)
            }
        }
    }
}

impl From<ContainerEndpoint> for PathBuf {
    fn from(e: ContainerEndpoint) -> Self {
        match e {
            ContainerEndpoint::UnixStream(p) | ContainerEndpoint::UnixDatagram(p) => p,
            ContainerEndpoint::UdpDatagram(a) => PathBuf::from(format!("udp://{}", a)),
            ContainerEndpoint::TcpListener(a) => PathBuf::from(format!("tcp-connect://{}", a)),
            ContainerEndpoint::TcpStream(a) => PathBuf::from(format!("tcp-listen://{}", a)),
        }
    }
}

impl<'a> TryFrom<&'a crate::server::NetworkEndpoint> for ContainerEndpoint {
    type Error = String;

    fn try_from(endpoint: &'a crate::server::NetworkEndpoint) -> Result<Self, Self::Error> {
        match endpoint {
            crate::server::NetworkEndpoint::UnixStream(s) => Ok(Self::UnixStream(PathBuf::from(s))),
            crate::server::NetworkEndpoint::UnixDatagram(s) => {
                Ok(Self::UnixDatagram(PathBuf::from(s)))
            }
            crate::server::NetworkEndpoint::UdpDatagram(s) => {
                Ok(Self::UdpDatagram(to_socket_addr(s)?))
            }
            crate::server::NetworkEndpoint::TcpListener(s) => {
                Ok(Self::TcpListener(to_socket_addr(s)?))
            }
            crate::server::NetworkEndpoint::TcpStream(s) => Ok(Self::TcpStream(to_socket_addr(s)?)),
        }
    }
}

impl TryFrom<url::Url> for ContainerEndpoint {
    type Error = String;

    fn try_from(url: url::Url) -> Result<Self, Self::Error> {
        match url.scheme() {
            "unix" => {
                let url = url.to_string().replacen("unix://", "", 1);
                Ok(Self::UnixStream(PathBuf::from(url)))
            }
            "udp" => {
                let url = url.to_string().replacen("udp://", "", 1);
                let addr: SocketAddr = url.parse().map_err(|e: AddrParseError| e.to_string())?;
                Ok(Self::UdpDatagram(addr))
            }
            "tcp-connect" => {
                let url = url.to_string().replacen("tcp-connect://", "", 1);
                let addr: SocketAddr = url.parse().map_err(|e: AddrParseError| e.to_string())?;
                Ok(Self::TcpStream(addr))
            }
            "tcp-listen" => Ok(Self::TcpListener(SocketAddr::new(
                Ipv4Addr::new(127, 0, 0, 1).into(),
                0,
            ))),
            scheme => Err(format!("Unknown scheme: {scheme}")),
        }
    }
}

fn to_socket_addr(s: &str) -> Result<SocketAddr, String> {
    s.parse().map_err(|e: AddrParseError| e.to_string())
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
