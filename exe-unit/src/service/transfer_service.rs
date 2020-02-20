use actix::prelude::*;
use std::path::Path;
use log::{info};

use super::transfers::Transfers;
use crate::message::Shutdown;



/// Handles resources transfers.
pub struct TransferService {
    transfers: Transfers,
}


impl Actor for TransferService {
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        info!("Transfers service started.");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("Transfers service stopped.");
    }
}

impl TransferService {
    pub fn new(workdir: &Path, cachedir: &Path) -> TransferService {
        TransferService{transfers: Transfers::new()}
    }
}


impl Handler<Shutdown> for TransferService {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}


