use actix::{Actor, Addr, Message};
use futures::channel::mpsc;
use std::marker::PhantomData;
use ya_client_model::vpn::CreateNetwork;
use ya_utils_networking::vpn::Error;

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnCreateNetwork {
    pub node_ip: String,
    pub net_id: String,
    pub net_ip: String,
    pub net_mask: String,
}

impl From<CreateNetwork> for VpnCreateNetwork {
    fn from(create: CreateNetwork) -> Self {
        Self {
            node_ip: create.node_ip,
            net_id: create.id,
            net_ip: create.ip,
            net_mask: create.mask,
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

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnAddAddress {
    pub net_id: String,
    pub ip: String,
}

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnAddNode {
    pub net_id: String,
    pub ip: String,
    pub id: String,
}

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub struct VpnRemoveNode {
    pub net_id: String,
    pub id: String,
}

#[derive(Message)]
#[rtype(result = "Result<mpsc::Receiver<Vec<u8>>, Error>")]
pub(crate) struct ConnectTcp {
    pub receiver: mpsc::Receiver<Vec<u8>>,
    pub ip: String,
    pub port: u16,
}

#[derive(Message)]
#[rtype(result = "Result<(), Error>")]
pub(crate) struct Shutdown;
