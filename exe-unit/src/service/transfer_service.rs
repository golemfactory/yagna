use actix::prelude::*;
use crate::Result;
use crate::error::Error;
use std::path::{Path, PathBuf};
use std::io::BufReader;
use std::fs::File;

use log::{info};
use url::Url;

use super::transfers::Cache;
use super::transfers::Transfers;
use super::transfers::{LocalTransfer, HttpTransfer};
use crate::message::Shutdown;
use thiserror::private::PathAsDisplay;


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
#[rtype(result = "Result<PathBuf>")]
pub struct DeployImage;

// =========================================== //
// TransferService implementation
// =========================================== //

/// Handles resources transfers.
pub struct TransferService {
    transfers: Transfers,
    workdir: PathBuf,
    cache: Cache,
    agreement: PathBuf,
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
    pub fn new(workdir: &Path, cachedir: &Path, agreement_path: &Path) -> TransferService {
        let mut transfers = Transfers::new();
        transfers.register_protocol(LocalTransfer::new());
        transfers.register_protocol(HttpTransfer::new());

        TransferService{
            transfers,
            workdir: workdir.to_path_buf(),
            cache: Cache::new(cachedir),
            agreement: agreement_path.to_path_buf(),
        }
    }

    fn load_package_url(agreement_file: &Path) -> Result<String> {
        let reader = BufReader::new(File::open(agreement_file)?);

        let json: serde_json::Value = serde_json::from_reader(reader)?;

        let package_field = "/golem.srv.comp.wasm.task_package";
        let package_value = json.pointer(package_field)
            .ok_or(Error::CommandError(format!("Agreement field '{}' doesn't exist.", package_field)))?;

        let package = package_value
            .as_str()
            .ok_or(Error::CommandError(format!("Agreement field '{}' is not string type.", package_field)))?
            .to_owned();
        return Ok(package);
    }

    fn deploy(&mut self, _msg: DeployImage) -> Result<PathBuf> {
        let package = TransferService::load_package_url(&self.agreement)?;

        info!("Deploying image [{}].", &package);

//        let image = Url::parse(&package)
//            .map_err(|error| Error::CommandError(format!("Can't parse package url [{}].", &package)))?;
        let (opt_image_hash, image_url) = Transfers::extract_hash(&package)?;

        match opt_image_hash {
            Option::None => Err(Error::CommandError(format!("Image hash required in deploy command."))),
            Option::Some(image_hash) => {
                info!("Trying to find image [{}] in cache.", &image_url);

                if let Some(image_path) = self.cache.find_in_cache(&image_url, &image_hash) {
                    info!("Image [{}] found in cache.", &image_path.display());
                    return Ok(image_path);
                }
                else {
                    info!("Image not found in cache. Downloading...");

                    let to = Url::parse(&format!("file:///{}", &image_hash.digest)).unwrap();
                    let dest_path = self.transfers.transfer(&image_url, &to, &self.cache.get_dir())?;

                    //TODO: Check hash. We should remove file on invalid hash.
                    //      Otherwise it will be loaded from cache next time.
                    info!("Validating image [{}] hash...", dest_path);
                    Transfers::validate_hash(&dest_path, &image_hash)?;
                    info!("Image [{}] is valid.", dest_path);

                    // If we are here, dest_path is correct.
                    Ok(dest_path.to_file_path().unwrap())
                }
            }
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(error: anyhow::Error) -> Self {
        Error::CommandError(format!("{}", error))
    }
}

// =========================================== //
// Messages handling
// =========================================== //

impl Handler<DeployImage> for TransferService {
    type Result = ActorResponse<Self, PathBuf, Error>;

    fn handle(&mut self, msg: DeployImage, ctx: &mut Self::Context) -> Self::Result {
        let path = self.deploy(msg)
            .map_err(|error| Error::CommandError(error.to_string()));

        return ActorResponse::reply(path);
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

        let response = self.transfers.transfer(&from.unwrap(), &to.unwrap(), &self.workdir)
            .map(|_| ())
            .map_err(|error| Error::CommandError(error.to_string()));

        info!("Transfer from [{}] to [{}] finished.", &msg.from, &msg.to);
        return ActorResponse::reply(response);
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


