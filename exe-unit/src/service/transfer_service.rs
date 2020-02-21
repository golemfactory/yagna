use actix::prelude::*;
use crate::Result;
use crate::error::Error;
use std::path::{Path, PathBuf};
use log::{info};
use url::Url;

use super::transfers::Transfers;
use super::transfers::{LocalTransfer, HttpTransfer};
use crate::message::Shutdown;


// =========================================== //
// Public exposed messages
// =========================================== //

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct TransferResource {
    pub from: String,
    pub to: String,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct DeployImage {
    pub image: String,
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
        let mut transfers = Transfers::new();
        transfers.register_protocol(LocalTransfer::new());
        transfers.register_protocol(HttpTransfer::new());

        TransferService{
            transfers,
            workdir: workdir.to_path_buf(),
            cachedir: cachedir.to_path_buf(),
        }
    }
}

impl Handler<TransferResource> for TransferService {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: TransferResource, ctx: &mut Self::Context) -> Self::Result {
        info!("Transfering resource from [{}] to [{}].", &msg.from, &msg.to);

        let from = Url::parse(&msg.from)
            .map_err(|error| Error::CommandError(format!("Can't parse source URL [{}]. Error: {}", &msg.from, error)));
        let to = Url::parse(&msg.to)
            .map_err(|error| Error::CommandError(format!("Can't parse destination URL [{}]. Error: {}", &msg.to, error)));

        if from.is_err() {
            return ActorResponse::reply(from.map(|_| ()));
        }

        if to.is_err() {
            return ActorResponse::reply(to.map(|_| ()));
        }

        //TODO: Check if paths are inside workdir
        ActorResponse::reply(self.transfers.transfer(&from.unwrap(), &to.unwrap(), &self.workdir)
            .map_err(|error| Error::CommandError(error.to_string())))
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


