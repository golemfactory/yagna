use actix::{Actor, Addr, Message};
use futures::channel::mpsc;
use smoltcp::socket::SocketHandle;
use std::marker::PhantomData;
use ya_client_model::net::{CreateNetwork, Network};
use ya_utils_networking::vpn::Error;

#[derive(Clone, Debug)]
pub enum DisconnectReason {
    SinkClosed,
    SocketClosed,
    ConnectionFailed,
}

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnCreateNetwork {
    pub network: Network,
    pub requestor_id: String,
    pub requestor_address: String,
}

impl VpnCreateNetwork {
    pub fn new(requestor_id: String, create: CreateNetwork) -> Self {
        Self {
            network: create.network,
            requestor_id,
            requestor_address: create.requestor_address,
        }
    }
}

#[derive(Message)]
#[rtype(result = "Result<Addr<T>, Error>")]
pub struct VpnGetNetwork<T: Actor> {
    pub net_id: String,
    pub phantom: PhantomData<T>,
}

impl<T: Actor> VpnGetNetwork<T> {
    pub fn new(net_id: String) -> Self {
        Self {
            net_id,
            phantom: PhantomData,
        }
    }
}

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnRemoveNetwork {
    pub net_id: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnAddAddress {
    pub net_id: String,
    pub address: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnAddNode {
    pub net_id: String,
    pub id: String,
    pub address: String,
}

#[derive(Debug, Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnRemoveNode {
    pub net_id: String,
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
