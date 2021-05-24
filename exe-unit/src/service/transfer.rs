use crate::deploy::ContainerVolume;
use crate::error::Error;
use crate::message::Shutdown;
use crate::util::path::{CachePath, ProjectedPath};
use crate::util::url::TransferUrl;
use crate::util::Abort;
use crate::{ExeUnitContext, Result};
use actix::prelude::*;
use futures::future::Abortable;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;
use ya_client_model::activity::TransferArgs;
use ya_transfer::error::Error as TransferError;
use ya_transfer::*;

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct TransferResource {
    pub from: String,
    pub to: String,
    pub args: TransferArgs,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct AddVolumes(Vec<ContainerVolume>);

impl AddVolumes {
    pub fn new(vols: Vec<ContainerVolume>) -> Self {
        AddVolumes(vols)
    }
}

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<Option<PathBuf>>")]
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

struct ContainerTransferProvider {
    file_tp: FileTransferProvider,
    dir_tp: DirTransferProvider,
    work_dir: PathBuf,
    vols: Vec<ContainerVolume>,
}

impl ContainerTransferProvider {
    fn new(work_dir: PathBuf, vols: Vec<ContainerVolume>) -> Self {
        ContainerTransferProvider {
            file_tp: Default::default(),
            dir_tp: Default::default(),
            work_dir,
            vols,
        }
    }

    fn resolve_path(&self, container_path: &str) -> std::result::Result<PathBuf, TransferError> {
        fn is_prefix_of(base: &str, path: &str) -> usize {
            if path.starts_with(base) && (path == base || path[base.len()..].starts_with("/")) {
                base.len() + 1
            } else {
                0
            }
        }

        if let Some((_, c)) = self
            .vols
            .iter()
            .map(|c| (is_prefix_of(&c.path, container_path), c))
            .max_by_key(|(prefix, _)| *prefix)
            .filter(|(prefix, _)| (*prefix) > 0)
        {
            let vol_base = self.work_dir.join(&c.name);

            if c.path == container_path {
                return Ok(vol_base);
            }

            let path = &container_path[c.path.len() + 1..];
            if path.starts_with("/") {
                return Err(TransferError::IoError(io::Error::new(
                    io::ErrorKind::NotFound,
                    anyhow::anyhow!("invalid path format: [{}]", container_path),
                )));
            }
            Ok(vol_base.join(path))
        } else {
            log::warn!("path not found in container: {}", container_path);
            Err(TransferError::IoError(io::Error::new(
                io::ErrorKind::NotFound,
                anyhow::anyhow!("path not found in container: {}", container_path),
            )))
        }
    }

    fn resolve_url(&self, path: &str) -> std::result::Result<Url, TransferError> {
        Ok(Url::from_file_path(self.resolve_path(path)?).unwrap())
    }
}

impl TransferProvider<TransferData, TransferError> for ContainerTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["container"]
    }

    fn source(
        &self,
        url: &Url,
        args: &TransferArgs,
    ) -> TransferStream<TransferData, TransferError> {
        let file_url = match self.resolve_url(url.path_decoded().as_str()) {
            Ok(v) => v,
            Err(e) => return TransferStream::err(e),
        };

        if args.format.is_some() {
            return self.dir_tp.source(&file_url, args);
        }
        self.file_tp.source(&file_url, args)
    }

    fn destination(
        &self,
        url: &Url,
        args: &TransferArgs,
    ) -> TransferSink<TransferData, TransferError> {
        let file_url = match self.resolve_url(url.path_decoded().as_str()) {
            Ok(v) => v,
            Err(e) => return TransferSink::err(e),
        };

        if args.format.is_some() {
            return self.dir_tp.destination(&file_url, args);
        }
        self.file_tp.destination(&file_url, args)
    }
}

/// Handles resources transfers.
pub struct TransferService {
    providers: HashMap<&'static str, Rc<dyn TransferProvider<TransferData, TransferError>>>,
    cache: Cache,
    work_dir: PathBuf,
    task_package: Option<String>,
    abort_handles: HashSet<Abort>,
}

impl TransferService {
    pub fn new(ctx: &ExeUnitContext) -> TransferService {
        TransferService {
            providers: Self::default_providers(),
            cache: Cache::new(ctx.cache_dir.clone()),
            work_dir: ctx.work_dir.clone(),
            task_package: ctx.agreement.task_package.clone(),
            abort_handles: HashSet::new(),
        }
    }

    pub fn schemes() -> Vec<String> {
        Self::default_providers()
            .values()
            .map(|p| p.schemes())
            .flatten()
            .collect::<HashSet<_>>()
            .into_iter()
            .map(ToString::to_string)
            .collect()
    }

    fn default_providers(
    ) -> HashMap<&'static str, Rc<dyn TransferProvider<TransferData, TransferError>>> {
        let mut providers = HashMap::new();

        let provider_vec: Vec<Rc<dyn TransferProvider<TransferData, TransferError>>> = vec![
            Rc::new(GftpTransferProvider::default()),
            Rc::new(HttpTransferProvider::default()),
        ];
        for provider in provider_vec {
            for scheme in provider.schemes() {
                providers.insert(scheme, provider.clone());
            }
        }
        providers
    }

    fn source(
        provider: Rc<dyn TransferProvider<TransferData, TransferError>>,
        transfer_url: &TransferUrl,
        args: &TransferArgs,
    ) -> std::result::Result<
        Box<dyn Stream<Item = std::result::Result<TransferData, TransferError>> + Unpin>,
        TransferError,
    > {
        let stream = provider.source(&transfer_url.url, args);
        match &transfer_url.hash {
            Some(hash) => Ok(Box::new(HashStream::try_new(
                stream,
                &hash.alg,
                hash.val.clone(),
            )?)),
            None => Ok(Box::new(stream)),
        }
    }

    fn provider(
        &self,
        transfer_url: &TransferUrl,
    ) -> Result<Rc<dyn TransferProvider<TransferData, TransferError>>> {
        let scheme = transfer_url.url.scheme();
        Ok(self
            .providers
            .get(scheme)
            .ok_or(TransferError::UnsupportedSchemeError(scheme.to_owned()))?
            .clone())
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
    type Result = ActorResponse<Self, Option<PathBuf>, Error>;

    #[allow(unused_variables)]
    fn handle(&mut self, _: DeployImage, ctx: &mut Self::Context) -> Self::Result {
        let image = match self.task_package.as_ref() {
            Some(image) => image,
            None => return ActorResponse::reply(Ok(None)),
        };

        let source_url = actor_try!(TransferUrl::parse_with_hash(image, "file"));
        let cache_name = actor_try!(Cache::name(&source_url));
        let final_path = self.cache.to_final_path(&cache_name);

        log::info!(
            "Deploying from {:?} to {:?}",
            source_url.url,
            final_path.to_path_buf()
        );

        let args = TransferArgs::default();
        #[cfg(not(feature = "sgx"))]
        {
            let from_provider = actor_try!(self.provider(&source_url));
            let to_provider: FileTransferProvider = Default::default();
            let cache_path = self.cache.to_cache_path(&cache_name);
            let temp_path = self.cache.to_temp_path(&cache_name);
            let temp_url = Url::from_file_path(temp_path.to_path_buf()).unwrap();

            let address = ctx.address();
            let (abort, reg) = Abort::new_pair();

            let fut = async move {
                let final_path = final_path.to_path_buf();
                let temp_path = temp_path.to_path_buf();
                let cache_path = cache_path.to_path_buf();

                let stream_fn = || Self::source(from_provider.clone(), &source_url, &args);
                let sink_fn = || to_provider.destination(&temp_url, &args);

                if cache_path.exists() {
                    log::info!("Deploying cached image: {:?}", cache_path);
                    std::fs::copy(cache_path, &final_path)?;
                    return Ok(Some(final_path));
                }

                {
                    let _guard = AbortHandleGuard::register(address, abort).await?;
                    Abortable::new(retry_transfer(stream_fn, sink_fn, Retry::default()), reg)
                        .await
                        .map_err(TransferError::from)??;
                }

                std::fs::rename(temp_path, &cache_path)?;
                std::fs::copy(cache_path, &final_path)?;

                log::info!("Deployment from {:?} finished", source_url.url);
                Ok(Some(final_path))
            };
            return ActorResponse::r#async(fut.into_actor(self));
        }

        #[cfg(feature = "sgx")]
        {
            let fut = async move {
                let final_path = final_path.to_path_buf();
                let resp = reqwest::get(source_url.url)
                    .await
                    .map_err(|e| Error::Other(e.to_string()))?;
                let bytes = resp
                    .bytes()
                    .await
                    .map_err(|e| Error::Other(e.to_string()))?;
                std::fs::write(&final_path, bytes)?;
                Ok(Some(final_path))
            };
            return ActorResponse::r#async(fut.into_actor(self));
        }
    }
}

impl Handler<TransferResource> for TransferService {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: TransferResource, ctx: &mut Self::Context) -> Self::Result {
        let from = actor_try!(TransferUrl::parse(&msg.from, "container"));
        let to = actor_try!(TransferUrl::parse(&msg.to, "container"));
        let from_provider = actor_try!(self.provider(&from));
        let to_provider = actor_try!(self.provider(&to));

        let (abort, reg) = Abort::new_pair();
        let address = ctx.address();
        let fut = async move {
            let stream_fn = || Self::source(from_provider.clone(), &from, &msg.args);
            let sink_fn = || to_provider.destination(&to.url, &msg.args);

            log::info!("Transferring {:?} to {:?}", from.url, to.url);
            {
                let _guard = AbortHandleGuard::register(address, abort).await?;
                Abortable::new(retry_transfer(stream_fn, sink_fn, Retry::default()), reg)
                    .await
                    .map_err(TransferError::from)??;
            }
            log::info!("Transfer of {:?} to {:?} finished", from.url, to.url);
            Ok(())
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl Handler<AddVolumes> for TransferService {
    type Result = Result<()>;

    fn handle(&mut self, msg: AddVolumes, _ctx: &mut Self::Context) -> Self::Result {
        log::info!("Adding volumes: {:?}", msg.0);
        let container_transfer_provider =
            ContainerTransferProvider::new(self.work_dir.clone(), msg.0);
        self.providers
            .insert("container", Rc::new(container_transfer_provider));
        Ok(())
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

struct AbortHandleGuard {
    address: Addr<TransferService>,
    abort: Abort,
}

impl AbortHandleGuard {
    pub async fn register(address: Addr<TransferService>, abort: Abort) -> Result<Self> {
        address.send(AddAbortHandle(abort.clone())).await?;
        Ok(AbortHandleGuard { address, abort })
    }
}

impl Drop for AbortHandleGuard {
    fn drop(&mut self) {
        self.address.do_send(RemoveAbortHandle(self.abort.clone()));
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

        let name = transfer_url.file_name()?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        Ok(CachePath::new(name.into(), hash.val.clone(), nonce))
    }

    #[inline(always)]
    #[cfg(not(feature = "sgx"))]
    fn to_temp_path(&self, path: &CachePath) -> ProjectedPath {
        ProjectedPath::local(self.tmp_dir.clone(), path.temp_path_buf())
    }

    #[inline(always)]
    #[cfg(not(feature = "sgx"))]
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

#[cfg(test)]
mod test {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_resolve_1() {
        let c = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![
                ContainerVolume {
                    name: "vol-3a9710d2-42f1-4502-9098-bc0bab9e7acc".into(),
                    path: "/in".into(),
                },
                ContainerVolume {
                    name: "vol-17599e4b-3aab-4fa8-b08d-440f48bd61e9".into(),
                    path: "/out".into(),
                },
            ],
        );
        assert_eq!(
            c.resolve_path("/in/task.json").unwrap(),
            std::path::Path::new("/tmp/vol-3a9710d2-42f1-4502-9098-bc0bab9e7acc/task.json")
        );
        assert_eq!(
            c.resolve_path("/out/task.json").unwrap(),
            std::path::Path::new("/tmp/vol-17599e4b-3aab-4fa8-b08d-440f48bd61e9/task.json")
        );
        assert!(c.resolve_path("/outs/task.json").is_err());
        assert!(c.resolve_path("/in//task.json").is_err());
        assert_eq!(
            c.resolve_path("/in").unwrap(),
            std::path::Path::new("/tmp/vol-3a9710d2-42f1-4502-9098-bc0bab9e7acc")
        );
    }

    #[test]
    fn test_resolve_2() {
        let c = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![
                ContainerVolume {
                    name: "vol-1".into(),
                    path: "/in/dst".into(),
                },
                ContainerVolume {
                    name: "vol-2".into(),
                    path: "/in".into(),
                },
                ContainerVolume {
                    name: "vol-3".into(),
                    path: "/out".into(),
                },
                ContainerVolume {
                    name: "vol-4".into(),
                    path: "/out/bin".into(),
                },
                ContainerVolume {
                    name: "vol-5".into(),
                    path: "/out/lib".into(),
                },
            ],
        );

        let check_resolve = |container_path, expected_result| {
            assert_eq!(
                c.resolve_path(container_path).unwrap(),
                Path::new(expected_result)
            )
        };

        check_resolve("/in/task.json", "/tmp/vol-2/task.json");
        check_resolve("/in/dst/smok.bin", "/tmp/vol-1/smok.bin");
        check_resolve("/out/b/x.png", "/tmp/vol-3/b/x.png");
        check_resolve("/out/bin/bash", "/tmp/vol-4/bash");
        check_resolve("/out/lib/libc.so", "/tmp/vol-5/libc.so");
    }

    // [ContainerVolume { name: "", path: "" }, ContainerVolume { name: "", path: "" }, ContainerVo
    //        │ lume { name: "", path: "" }]
    #[test]
    fn test_resolve_3() {
        let c = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![
                ContainerVolume {
                    name: "vol-bd959639-9148-4d7c-8ba2-05a654e84476".into(),
                    path: "/golem/output".into(),
                },
                ContainerVolume {
                    name: "vol-4d59d1d6-2571-4ab8-a86a-b6199a9a1f4b".into(),
                    path: "/golem/resource".into(),
                },
                ContainerVolume {
                    name: "vol-b51194da-2fce-45b7-bff8-37e4ef8f7535".into(),
                    path: "/golem/work".into(),
                },
            ],
        );

        let check_resolve = |container_path, expected_result| {
            assert_eq!(
                c.resolve_path(container_path).unwrap(),
                Path::new(expected_result)
            )
        };

        check_resolve(
            "/golem/resource/scene.blend",
            "/tmp/vol-4d59d1d6-2571-4ab8-a86a-b6199a9a1f4b/scene.blend",
        );
    }

    #[test]
    fn test_resolve_compat() {
        let c = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![ContainerVolume {
                name: ".".into(),
                path: "".into(),
            }],
        );
        eprintln!("{}", c.resolve_path("/in/tasks.json").unwrap().display());
    }
}
