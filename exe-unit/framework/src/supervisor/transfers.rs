use super::protocols::transfer_protocol::TransferProtocol;

use actix::prelude::*;


/// Actor responsible for managing transfers. It should forward
/// transfer requests to implementation of different transfer protocols.
pub struct Transfers {
    protocols: Vec<Box<dyn TransferProtocol>>,
}

impl Transfers {
    pub fn new() -> Transfers {
        Transfers{protocols: vec![]}
    }
}


// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for Transfers {
    type Context = Context<Self>;
}





