use std::net::{AddrParseError, IpAddr};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid IP address: {0}")]
    IpAddrInvalid(#[from] AddrParseError),
    #[error("IP address not allowed: {0}")]
    IpAddrNotAllowed(IpAddr),
    #[error("Invalid IP network address: {0}")]
    NetAddrInvalid(String),
    #[error("Packet malformed: {0}")]
    PacketMalformed(String),
    #[error("Protocol not supported: {0}")]
    ProtocolNotSupported(String),
    #[error("{0}")]
    Other(String),
}
