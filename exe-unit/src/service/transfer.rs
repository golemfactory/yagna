use crate::error::Error;
use crate::message::Shutdown;
use crate::util::path::{CachePath, ProjectedPath};
use crate::util::url::TransferUrl;
use crate::util::Abort;
use crate::{ExeUnitContext, Result};
use actix::prelude::*;
use futures::future::{AbortHandle, Abortable};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use ya_transfer::error::Error as TransferError;
use ya_transfer::{
    transfer, FileTransferProvider, GftpTransferProvider, HashStream, HttpTransferProvider,
    TransferData, TransferProvider, TransferSink,
};

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
#[rtype(result = "()")]
pub struct AbortTransfers;

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
    task_package: String,
    abort_handles: HashSet<Abort>,
}

impl TransferService {
    pub fn new(ctx: &ExeUnitContext) -> TransferService {
        let mut providers = HashMap::new();

        let provider_vec: Vec<Rc<Box<dyn TransferProvider<TransferData, TransferError>>>> = vec![
            Rc::new(Box::new(GftpTransferProvider::default())),
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
            work_dir: ctx.work_dir.clone(),
            task_package: ctx.agreement.task_package.clone(),
            abort_handles: HashSet::new(),
        }
    }

    fn source(
        &self,
        transfer_url: &TransferUrl,
    ) -> Result<Box<dyn Stream<Item = std::result::Result<TransferData, TransferError>> + Unpin>>
    {
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
                "container" => Ok(ProjectedPath::local(work_dir, path.into())
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
                    let projected = ProjectedPath::local(work_dir, path.into());
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
}

impl Actor for TransferService {
    type Context = Context<Self>;

    fn started(&mut self, _: &mut Self::Context) {
        log::info!("Transfer service started");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::info!("Transfer service stopped");
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

    fn handle(&mut self, _: DeployImage, ctx: &mut Self::Context) -> Self::Result {
        let source_url = actor_try!(TransferUrl::parse_with_hash(&self.task_package, "file"));
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
        let dest = actor_try!(self.destination(&cache_url));

        let address = ctx.address();
        let (handle, reg) = AbortHandle::new_pair();
        let abort = Abort::from(handle);

        let fut = async move {
            let final_path = final_path.to_path_buf();
            if final_path.exists() {
                return Ok(final_path);
            }

            address.send(AddAbortHandle(abort.clone())).await?;
            Abortable::new(transfer(source, dest), reg)
                .await
                .map_err(TransferError::from)??;
            address.send(RemoveAbortHandle(abort)).await?;

            std::fs::rename(cache_path.to_path_buf(), &final_path)?;

            log::info!("Deployment from {:?} finished", source_url.url);
            Ok(final_path)
        };

        return ActorResponse::r#async(fut.into_actor(self));
    }
}

impl Handler<TransferResource> for TransferService {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: TransferResource, ctx: &mut Self::Context) -> Self::Result {
        let address = ctx.address();
        let from = actor_try!(self.parse_from(&msg.from));
        let to = actor_try!(self.parse_to(&msg.to));

        log::info!("Transferring {:?} to {:?}", from.url, to.url);

        let source = actor_try!(self.source(&from));
        let dest = actor_try!(self.destination(&to));

        let (handle, reg) = AbortHandle::new_pair();
        let abort = Abort::from(handle);

        return ActorResponse::r#async(
            async move {
                address.send(AddAbortHandle(abort.clone())).await?;
                Abortable::new(transfer(source, dest), reg)
                    .await
                    .map_err(TransferError::from)??;
                address.send(RemoveAbortHandle(abort)).await?;

                log::info!("Transfer of {:?} to {:?} finished", from.url, to.url);
                Ok(())
            }
            .into_actor(self),
        );
    }
}

impl Handler<AddAbortHandle> for TransferService {
    type Result = <AddAbortHandle as Message>::Result;

    fn handle(&mut self, msg: AddAbortHandle, _: &mut Self::Context) -> Self::Result {
        self.abort_handles.insert(msg.0);
    }
}

impl Handler<RemoveAbortHandle> for TransferService {
    type Result = <RemoveAbortHandle as Message>::Result;

    fn handle(&mut self, msg: RemoveAbortHandle, _: &mut Self::Context) -> Self::Result {
        self.abort_handles.remove(&msg.0);
    }
}

impl Handler<AbortTransfers> for TransferService {
    type Result = <AbortTransfers as Message>::Result;

    fn handle(&mut self, _: AbortTransfers, _: &mut Self::Context) -> Self::Result {
        for handle in std::mem::replace(&mut self.abort_handles, HashSet::new()).into_iter() {
            handle.abort();
        }
    }
}

impl Handler<Shutdown> for TransferService {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.address().do_send(AbortTransfers {});
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
