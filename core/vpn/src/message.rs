use crate::Result;
use actix::{Message, Recipient};
use futures::channel::mpsc;
use smoltcp::iface::SocketHandle;
use smoltcp::wire::IpEndpoint;
use ya_client_model::net::*;
use ya_utils_networking::vpn::{Error, Protocol};

#[derive(Debug, Message)]
#[rtype(result = "Result<Vec<Address>>")]
pub struct GetAddresses;

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct AddAddress {
    pub address: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<Vec<Node>>")]
pub struct GetNodes;

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct AddNode {
    pub id: String,
    pub address: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct RemoveNode {
    pub id: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<Vec<Connection>>")]
pub struct GetConnections;

#[derive(Message)]
#[rtype(result = "Result<UserConnection>")]
pub struct Connect {
    pub protocol: Protocol,
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Disconnect {
    pub handle: SocketHandle,
    pub reason: DisconnectReason,
}

impl Disconnect {
    pub fn new(handle: SocketHandle, reason: DisconnectReason) -> Self {
        Self { handle, reason }
    }

    pub fn with(handle: SocketHandle, err: &Error) -> Self {
        Self::new(
            handle,
            match &err {
                Error::Cancelled => DisconnectReason::SinkClosed,
                Error::ConnectionTimeout => DisconnectReason::ConnectionTimeout,
                _ => DisconnectReason::ConnectionFailed,
            },
        )
    }
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct Packet {
    pub data: Vec<u8>,
    pub meta: ConnectionMeta,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown;

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct DataSent;

#[derive(Clone, Debug)]
pub struct ConnectionMeta {
    pub handle: SocketHandle,
    pub protocol: Protocol,
    pub remote: IpEndpoint,
}

#[derive(Debug)]
pub struct UserConnection {
    pub vpn: Recipient<Packet>,
    pub rx: mpsc::Receiver<Vec<u8>>,
    pub meta: ConnectionMeta,
}

#[derive(Clone, Debug)]
pub enum DisconnectReason {
    SinkClosed,
    SocketClosed,
    ConnectionFailed,
    ConnectionTimeout,
}

impl std::fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
