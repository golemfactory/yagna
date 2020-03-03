use crate::error::Error;
use crate::message::Shutdown;
use crate::util::path::{CachePath, ProjectedPath};
use crate::util::url::TransferUrl;
use crate::util::Abort;
use crate::{ExeUnitContext, Result};
use actix::prelude::*;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use ya_transfer::error::Error as TransferError;
use ya_transfer::file::FileTransferProvider;
use ya_transfer::http::HttpTransferProvider;
use ya_transfer::{transfer, HashStream, TransferData, TransferProvider, TransferSink};

type TransferResult<T> = std::result::Result<T, TransferError>;

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct TransferResource {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<PathBuf>")]
pub struct DeployImage;

#[derive(Clone, Debug, Message)]
#[rtype("()")]
struct AddAbortHandle(Abort);

#[derive(Clone, Debug, Message)]
#[rtype("()")]
struct RemoveAbortHandle(Abort);

/// Handles resources transfers.
pub struct TransferService {
    providers: HashMap<&'static str, Rc<Box<dyn TransferProvider<TransferData, TransferError>>>>,
    cache: Cache,
    work_dir: PathBuf,
    agreement: PathBuf,
    abort_handles: Vec<Abort>,
}

impl TransferService {
    pub fn new(ctx: ExeUnitContext) -> TransferService {
        let mut providers = HashMap::new();

        let provider_vec: Vec<Rc<Box<dyn TransferProvider<TransferData, TransferError>>>> = vec![
            Rc::new(Box::new(HttpTransferProvider::default())),
            Rc::new(Box::new(FileTransferProvider::default())),
        ];
        for provider in provider_vec.into_iter() {
            for scheme in provider.schemes().iter() {
                providers.insert(*scheme, provider.clone());
            }
        }

        TransferService {
            providers,
            cache: Cache::new(ctx.cache_dir.clone()),
            work_dir: ctx.work_dir,
            agreement: ctx.agreement,
            abort_handles: Vec::new(),
        }
    }

    fn source(
        &self,
        transfer_url: &TransferUrl,
    ) -> Result<Box<dyn Stream<Item = TransferResult<TransferData>> + Unpin>> {
        let scheme = transfer_url.url.scheme();
        let provider = self
            .providers
            .get(scheme)
            .ok_or(TransferError::UnsupportedSchemeError(scheme.to_owned()))?;

        let stream = provider.source(&transfer_url.url);
        match &transfer_url.hash {
            Some(hash) => Ok(Box::new(HashStream::try_new(
                stream,
                &hash.alg,
                hash.val.clone(),
            )?)),
            None => Ok(Box::new(stream)),
        }
    }

    fn destination(
        &self,
        transfer_url: &TransferUrl,
    ) -> Result<TransferSink<TransferData, TransferError>> {
        let scheme = transfer_url.url.scheme();

        let provider = self
            .providers
            .get(scheme)
            .ok_or(TransferError::UnsupportedSchemeError(scheme.to_owned()))?;

        Ok(provider.destination(&transfer_url.url))
    }

    fn parse_from(&self, from: &str) -> Result<TransferUrl> {
        let work_dir = self.work_dir.clone();
        Ok(TransferUrl::parse(from, "container")?
            .map_path(|scheme, path| match scheme {
                "container" => Ok(ProjectedPath::container(path.into())
                    .to_local(work_dir)
                    .to_path_buf()
                    .to_str()
                    .unwrap()
                    .to_owned()),
                _ => Ok(path.to_owned()),
            })?
            .map_scheme(|scheme| match scheme {
                "container" => "file",
                _ => scheme,
            })?)
    }

    fn parse_to(&self, to: &str) -> Result<TransferUrl> {
        let work_dir = self.work_dir.clone();

        Ok(TransferUrl::parse(to, "container")?
            .map_path(|scheme, path| match scheme {
                "container" => {
                    let projected = ProjectedPath::container(path.into()).to_local(work_dir);
                    projected.create_dir_all().map_err(TransferError::from)?;
                    Ok(projected.to_path_buf().to_str().unwrap().to_owned())
                }
                _ => Ok(path.to_owned()),
            })?
            .map_scheme(|scheme| match scheme {
                "container" => "file",
                _ => scheme,
            })?)
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

macro_rules! actor_try {
    ($expr:expr) => {
        match $expr {
            Ok(val) => val,
            Err(err) => {
                return ActorResponse::reply(Err(Error::from(err)));
            }
        }
    };
    ($expr:expr,) => {
        $crate::actor_try!($expr)
    };
}

impl Handler<DeployImage> for TransferService {
    type Result = ActorResponse<Self, PathBuf, Error>;

    fn handle(&mut self, _: DeployImage, _: &mut Self::Context) -> Self::Result {
        let pkg_url = actor_try!(TransferService::load_package_url(&self.agreement));
        let source_url = actor_try!(TransferUrl::parse(&pkg_url, "file"));
        let cache_name = actor_try!(Cache::name(&source_url));
        let cache_path = self.cache.to_cache_path(&cache_name);
        let final_path = self.cache.to_final_path(&cache_name);
        let cache_url = actor_try!(TransferUrl::try_from(cache_path.clone()));

        log::info!(
            "Deploying from {:?} to {:?}",
            source_url.url,
            final_path.to_path_buf()
        );

        let source = actor_try!(self.source(&source_url));
        let destination = actor_try!(self.destination(&cache_url));

        let fut = async move {
            let final_path = final_path.to_path_buf();
            if final_path.exists() {
                return Ok(final_path);
            }

            transfer(source, destination).await?;
            log::debug!("Deployment from {:?} finished", source_url.url);

            std::fs::rename(cache_path.to_path_buf(), &final_path)?;

            Ok(final_path)
        };

        return ActorResponse::r#async(fut.into_actor(self));
    }
}

impl Handler<TransferResource> for TransferService {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: TransferResource, _: &mut Self::Context) -> Self::Result {
        let from = actor_try!(self.parse_from(&msg.from));
        let to = actor_try!(self.parse_to(&msg.to));

        log::info!("Transferring {:?} to {:?}", from.url, to.url);

        let source = actor_try!(self.source(&from));
        let destination = actor_try!(self.destination(&to));

        return ActorResponse::r#async(
            async move {
                transfer(source, destination).await?;
                Ok(())
            }
            .into_actor(self),
        );
    }
}

impl Handler<AddAbortHandle> for TransferService {
    type Result = <AddAbortHandle as Message>::Result;

    fn handle(&mut self, msg: AddAbortHandle, _: &mut Self::Context) -> Self::Result {
        self.abort_handles.push(msg.0);
    }
}

impl Handler<RemoveAbortHandle> for TransferService {
    type Result = <RemoveAbortHandle as Message>::Result;

    fn handle(&mut self, msg: RemoveAbortHandle, _: &mut Self::Context) -> Self::Result {
        if let Some(idx) = self.abort_handles.iter().position(|c| c == &msg.0) {
            self.abort_handles.remove(idx);
        }
    }
}

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

#[derive(Debug, Clone)]
struct Cache {
    dir: PathBuf,
    tmp_dir: PathBuf,
}

impl Cache {
    fn new(dir: PathBuf) -> Self {
        let tmp_dir = dir.clone().join("tmp");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        Cache { dir, tmp_dir }
    }

    fn name(transfer_url: &TransferUrl) -> Result<CachePath> {
        let hash = match &transfer_url.hash {
            Some(hash) => hash,
            None => return Err(TransferError::InvalidUrlError("hash required".to_owned()).into()),
        };

        let path = transfer_url.url.path();
        let name = match path.rfind("/") {
            Some(idx) => {
                if idx + 1 < path.len() - 1 {
                    &path[idx + 1..]
                } else {
                    path
                }
            }
            None => path,
        };

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        Ok(CachePath::new(name.into(), hash.val.clone(), nonce))
    }

    #[inline(always)]
    fn to_cache_path(&self, path: &CachePath) -> ProjectedPath {
        ProjectedPath::local(self.tmp_dir.clone(), path.cache_path_buf())
    }

    #[inline(always)]
    fn to_final_path(&self, path: &CachePath) -> ProjectedPath {
        ProjectedPath::local(self.dir.clone(), path.final_path_buf())
    }
}

impl TryFrom<ProjectedPath> for TransferUrl {
    type Error = Error;

    fn try_from(value: ProjectedPath) -> Result<Self> {
        TransferUrl::parse(
            value
                .to_path_buf()
                .to_str()
                .ok_or(Error::local(TransferError::InvalidUrlError(
                    "Invalid path".to_owned(),
                )))?,
            "file",
        )
        .map_err(Error::local)
    }
}
