use actix::Message;
use futures::channel::mpsc;
use smoltcp::socket::SocketHandle;
use ya_utils_networking::vpn::Error;

#[derive(Debug, Message)]
#[rtype(result = "Result<(), Error>")]
pub(crate) struct AddAddress {
    pub address: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<(), Error>")]
pub(crate) struct AddNode {
    pub id: String,
    pub address: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<(), Error>")]
pub(crate) struct RemoveNode {
    pub id: String,
}

#[derive(Message)]
#[rtype(result = "Result<mpsc::Receiver<Vec<u8>>, Error>")]
pub(crate) struct ConnectTcp {
    pub receiver: mpsc::Receiver<Vec<u8>>,
    pub address: String,
    pub port: u16,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<(), Error>")]
pub(crate) struct Disconnect {
    pub handle: SocketHandle,
    pub reason: DisconnectReason,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<(), Error>")]
pub(crate) struct Shutdown;

#[derive(Clone, Debug)]
pub enum DisconnectReason {
    SinkClosed,
    SocketClosed,
    ConnectionFailed,
    ConnectionTimeout,
}
