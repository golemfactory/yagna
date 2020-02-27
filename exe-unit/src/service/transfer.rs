use crate::error::Error;
use crate::message::Shutdown;
use crate::{ExeUnitContext, Result};
use actix::prelude::*;
use diesel::ExpressionMethods;
use futures::future::AbortHandle;
use std::fs::File;
use std::io::BufReader;
use std::path::{Component, Path, PathBuf};
use url::Url;
use ya_transfer::error::Error as TransferError;
use ya_transfer::file::FileTransferProvider;
use ya_transfer::http::HttpTransferProvider;
use ya_transfer::url::TransferLocation;
use ya_transfer::TransferProvider;

// =========================================== //
// Public exposed messages
// =========================================== //

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct TransferResource {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<PathBuf>")]
pub struct DeployImage;

// =========================================== //
// TransferService implementation
// =========================================== //

/// Handles resources transfers.
pub struct TransferService {
    providers: Vec<Box<dyn TransferProvider<TransferData, Error>>>,
    work_dir: PathBuf,
    cache_dir: PathBuf,
    agreement: PathBuf,
    abort_handles: Vec<AbortHandle>,
}

impl TransferService {
    pub fn new(ctx: ExeUnitContext) -> TransferService {
        TransferService {
            providers: vec![
                Box::new(HttpTransferProvider::default()),
                Box::new(FileTransferProvider::default()),
            ],
            work_dir: ctx.work_dir,
            cache_dir: ctx.cache_dir,
            agreement: ctx.agreement,
            abort_handles: Vec::new(),
        }
    }

    fn load_package_url(agreement_file: &Path) -> Result<String> {
        let reader = BufReader::new(File::open(agreement_file)?);

        let json: serde_json::Value = serde_json::from_reader(reader)?;

        let package_field = "/golem.srv.comp.wasm.task_package";
        let package_value = json
            .pointer(package_field)
            .ok_or(Error::CommandError(format!(
                "Agreement field '{}' doesn't exist.",
                package_field
            )))?;

        let package = package_value
            .as_str()
            .ok_or(Error::CommandError(format!(
                "Agreement field '{}' is not string type.",
                package_field
            )))?
            .to_owned();
        return Ok(package);
    }

    fn deploy(&mut self, _msg: DeployImage) -> Result<PathBuf> {
        let raw_url = TransferService::load_package_url(&self.agreement)?;
        let from = TransferLocation::parse(&raw_url)?;

        // match from {
        //     TransferLocation::WithHash { .. } => from,
        // }

        if let TransferLocation::Plain(_) = from {
            return Err(TransferError::InvalidUrlError("hash required in URL".into_owned()).into());
        }

        //        let image = Url::parse(&package)
        //            .map_err(|error| Error::CommandError(format!("Can't parse package url [{}].", &package)))?;
        let (opt_image_hash, image_url) = Transfers::extract_hash(&package)?;

        match opt_image_hash {
            Option::None => Err(Error::CommandError(format!(
                "Image hash required in deploy command."
            ))),
            Option::Some(image_hash) => {
                log::info!("Trying to find image [{}] in cache.", &image_url);

                if let Some(image_path) = self.cache.find_in_cache(&image_url, &image_hash) {
                    log::info!("Image [{}] found in cache.", &image_path.display());
                    return Ok(image_path);
                } else {
                    log::info!("Image not found in cache. Downloading...");

                    let to = Url::parse(&format!("file:///{}", &image_hash.digest)).unwrap();
                    let dest_path =
                        self.transfers
                            .transfer(&image_url, &to, &self.cache.get_dir())?;

                    //TODO: Check hash. We should remove file on invalid hash.
                    //      Otherwise it will be loaded from cache next time.
                    log::info!("Validating image [{}] hash...", dest_path);
                    Transfers::validate_hash(&dest_path, &image_hash)?;
                    log::info!("Image [{}] is valid.", dest_path);

                    // If we are here, dest_path is correct.
                    Ok(dest_path.to_file_path().unwrap())
                }
            }
        }
    }
}

impl Actor for TransferService {
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        log::info!("Transfer service started.");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("Transfer service stopped.");
    }
}

impl Handler<DeployImage> for TransferService {
    type Result = ActorResponse<Self, PathBuf, Error>;

    fn handle(&mut self, msg: DeployImage, ctx: &mut Self::Context) -> Self::Result {
        let path = self
            .deploy(msg)
            .map_err(|error| Error::CommandError(error.to_string()));

        return ActorResponse::reply(path);
    }
}

impl Handler<TransferResource> for TransferService {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: TransferResource, ctx: &mut Self::Context) -> Self::Result {
        let (from, to) = (msg.from, msg.to);

        log::info!("Transferring resource from [{}] to [{}].", from, to);

        let from = Url::parse(&msg.from).map_err(|error| {
            Error::CommandError(format!(
                "Can't parse source URL [{}]. Error: {}",
                &msg.from, error
            ))
        });
        let to = Url::parse(&msg.to).map_err(|error| {
            Error::CommandError(format!(
                "Can't parse destination URL [{}]. Error: {}",
                &msg.to, error
            ))
        });

        if from.is_err() {
            return ActorResponse::reply(from.map(|_| ()));
        }

        if to.is_err() {
            return ActorResponse::reply(to.map(|_| ()));
        }

        let response = self
            .transfers
            .transfer(&from.unwrap(), &to.unwrap(), &self.workdir)
            .map(|_| ())
            .map_err(|error| Error::CommandError(error.to_string()));

        log::info!("Transfer from [{}] to [{}] finished.", &msg.from, &msg.to);
        return ActorResponse::reply(response);
    }
}

// =========================================== //
// Implement Service interface
// =========================================== //

impl Handler<Shutdown> for TransferService {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        for handle in std::mem::replace(&mut self.abort_handles, Vec::new()).into_iter() {
            handle.abort();
        }
        ctx.stop();
        Ok(())
    }
}

#[derive(Clone, Debug)]
enum ProjectedPath {
    Local { dir: PathBuf, path: PathBuf },
    Container { path: PathBuf },
}

impl ProjectedPath {
    fn flatten(path: PathBuf) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                Component::CurDir => continue,
                Component::ParentDir => components.pop(),
                _ => components.push(component),
            }
        }
        components().into_iter().collect::<PathBuf>()
    }

    fn local(dir: PathBuf, path: PathBuf) -> Self {
        ProjectedPath::Local {
            dir,
            path: Self::flatten(path),
        }
    }

    fn container(path: PathBuf) -> Self {
        ProjectedPath::Container {
            path: Self::flatten(path),
        }
    }
}

impl ProjectedPath {
    fn create_dir_all(&self) -> Result<()> {
        if let ProjectedPath::Container { .. } = &self {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into());
        }

        let path = self.to_path_buf();
        if let Err(error) = std::fs::create_dir_all(path.parent()) {
            match &error.kind() {
                std::io::ErrorKind::AlreadyExists => (),
                _ => return Err(error.into()),
            }
        }

        Ok(())
    }

    fn to_path_buf(&self) -> PathBuf {
        match self {
            ProjectedPath::Local { dir, path } => dir.clone().join(path),
            ProjectedPath::Container { path } => path.clone(),
        }
    }

    fn to_local(&self, dir: PathBuf) -> Self {
        match self {
            ProjectedPath::Local { dir: _, path } => ProjectedPath::Local {
                dir,
                path: path.clone(),
            },
            ProjectedPath::Container(path) => ProjectedPath::Local {
                dir,
                path: path.clone(),
            },
        }
    }

    fn to_container(&self) -> Self {
        match self {
            ProjectedPath::Local { dir: _, path } => {
                ProjectedPath::Container { path: path.clone() }
            }
            ProjectedPath::Container(path) => ProjectedPath::Container { path: path.clone() },
        }
    }

    fn file_name(&self) -> Self {}
}
