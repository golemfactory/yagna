use std::net::IpAddr;
use crate::Result;
use actix::{Message, Recipient};
use futures::channel::mpsc;
use ya_client_model::net::*;
use ya_utils_networking::vpn::{
    stack::{
        connection::{Connection, ConnectionMeta},
        EgressEvent, IngressEvent,
    },
    Protocol, SocketDesc,
};

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
#[rtype(result = "Result<UserTcpConnection>")]
pub struct ConnectTcp {
    pub protocol: Protocol,
    pub address: String,
    pub port: u16,
}

#[derive(Message)]
#[rtype(result = "Result<UserRawConnection>")]
pub struct ConnectRaw {
    pub src_addr: IpAddr,
    pub dst_addr: IpAddr,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Disconnect {
    pub desc: SocketDesc,
    pub reason: DisconnectReason,
}

impl Disconnect {
    pub fn new(desc: SocketDesc, reason: DisconnectReason) -> Self {
        Self { desc, reason }
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

#[derive(Debug)]
pub struct UserTcpConnection {
    pub vpn: Recipient<Packet>,
    pub rx: mpsc::Receiver<Vec<u8>>,
    pub stack_connection: Connection,
}

#[derive(Debug)]
pub struct UserRawConnection {
    pub rx: mpsc::Receiver<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub enum DisconnectReason {
    SinkClosed,
    SocketClosed,
    ConnectionError,
}

impl std::fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Ingress {
    pub event: IngressEvent,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct Egress {
    pub event: EgressEvent,
}
