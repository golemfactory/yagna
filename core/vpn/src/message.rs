use crate::Result;
use actix::Message;
use futures::channel::mpsc;
use smoltcp::socket::SocketHandle;
use ya_client_model::net::*;

#[derive(Debug, Message)]
#[rtype(result = "Result<Vec<Address>>")]
pub(crate) struct GetAddresses;

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub(crate) struct AddAddress {
    pub address: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<Vec<Node>>")]
pub(crate) struct GetNodes;

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub(crate) struct AddNode {
    pub id: String,
    pub address: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub(crate) struct RemoveNode {
    pub id: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<Vec<Connection>>")]
pub(crate) struct GetConnections;

#[derive(Message)]
#[rtype(result = "Result<mpsc::Receiver<Vec<u8>>>")]
pub(crate) struct ConnectTcp {
    pub receiver: mpsc::Receiver<Vec<u8>>,
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub(crate) struct Disconnect {
    pub handle: SocketHandle,
    pub reason: DisconnectReason,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<()>")]
pub(crate) struct Shutdown;

#[derive(Clone, Debug)]
pub enum DisconnectReason {
    SinkClosed,
    SocketClosed,
    ConnectionFailed,
    ConnectionTimeout,
}
