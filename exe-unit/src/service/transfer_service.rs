use actix::prelude::*;
use anyhow::{Result, Error};
use std::path::{Path, PathBuf};
use log::{info};

use super::transfers::Transfers;
use crate::message::Shutdown;


// =========================================== //
// Public exposed messages
// =========================================== //

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct TransferResource {
    from: String,
    to: String,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct DeployImage {
    image: String,
}

// =========================================== //
// TransferService implementation
// =========================================== //

/// Handles resources transfers.
pub struct TransferService {
    transfers: Transfers,
    workdir: PathBuf,
    cachedir: PathBuf,
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
        TransferService{
            transfers: Transfers::new(),
            workdir: workdir.to_path_buf(),
            cachedir: cachedir.to_path_buf(),
        }
    }
}

impl Handler<TransferResource> for TransferService {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: TransferResource, ctx: &mut Self::Context) -> Self::Result {

        //TODO: Check if paths are inside workdir
        ActorResponse::reply(self.transfers.transfer(&msg.from, &msg.to))
    }
}


// =========================================== //
// Implement Service interface
// =========================================== //

impl Handler<Shutdown> for TransferService {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}


