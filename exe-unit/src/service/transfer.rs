use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use actix::prelude::*;
use futures::future::Abortable;
use url::Url;

use crate::deploy::ContainerVolume;
use crate::error::Error;
use crate::message::Shutdown;
use crate::util::cache::Cache;
use crate::util::Abort;
use crate::{ExeUnitContext, Result};

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
        ctx: &TransferContext,
    ) -> TransferStream<TransferData, TransferError> {
        let file_url = match self.resolve_url(url.path_decoded().as_str()) {
            Ok(v) => v,
            Err(e) => return TransferStream::err(e),
        };

        if ctx.args.format.is_some() {
            return self.dir_tp.source(&file_url, ctx);
        }
        self.file_tp.source(&file_url, ctx)
    }

    fn destination(
        &self,
        url: &Url,
        ctx: &TransferContext,
    ) -> TransferSink<TransferData, TransferError> {
        let file_url = match self.resolve_url(url.path_decoded().as_str()) {
            Ok(v) => v,
            Err(e) => return TransferSink::err(e),
        };

        if ctx.args.format.is_some() {
            return self.dir_tp.destination(&file_url, ctx);
        }
        self.file_tp.destination(&file_url, ctx)
    }
}

/// Handles resources transfers.
pub struct TransferService {
    providers: HashMap<&'static str, Rc<dyn TransferProvider<TransferData, TransferError>>>,
    cache: Cache,
    work_dir: PathBuf,
    task_package: Option<String>,
    abort_handles: Rc<RefCell<HashSet<Abort>>>,
}

impl TransferService {
    pub fn new(ctx: &ExeUnitContext) -> TransferService {
        TransferService {
            providers: Self::default_providers(),
            cache: Cache::new(ctx.cache_dir.clone()),
            work_dir: ctx.work_dir.clone(),
            task_package: ctx.agreement.task_package.clone(),
            abort_handles: Default::default(),
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

        let src_url = actor_try!(TransferUrl::parse_with_hash(image, "file"));
        let src_name = actor_try!(Cache::name(&src_url));
        let path = self.cache.to_final_path(&src_name).to_path_buf();

        log::info!("Deploying from {:?} to {:?}", src_url.url, path);

        #[cfg(not(feature = "sgx"))]
        {
            let path_tmp = self.cache.to_temp_path(&src_name).to_path_buf();

            let src = actor_try!(self.provider(&src_url));
            let dst: Rc<FileTransferProvider> = Default::default();
            let dst_url = TransferUrl {
                url: Url::from_file_path(&path_tmp).unwrap(),
                hash: None,
            };

            let handles = self.abort_handles.clone();
            let fut = async move {
                if path.exists() {
                    log::info!("Deploying cached image: {:?}", path);
                    return Ok(Some(path));
                }

                let (abort, reg) = Abort::new_pair();
                {
                    let ctx = Default::default();
                    let retry = transfer_with(src, &src_url, dst, &dst_url, &ctx);

                    let _guard = AbortHandleGuard::register(handles, abort);
                    Ok::<_, Error>(
                        Abortable::new(retry, reg)
                            .await
                            .map_err(TransferError::from)?
                            .map_err(|err| {
                                if let TransferError::InvalidHashError { .. } = err {
                                    let _ = std::fs::remove_file(&path_tmp);
                                }
                                err
                            })?,
                    )
                }?;

                move_file(&path_tmp, &path).await?;
                log::info!("Deployment from {:?} finished", src_url.url);

                Ok(Some(path))
            };
            return ActorResponse::r#async(fut.into_actor(self));
        }

        #[cfg(feature = "sgx")]
        {
            let fut = async move {
                let resp = reqwest::get(src_url.url)
                    .await
                    .map_err(|e| Error::Other(e.to_string()))?;
                let bytes = resp
                    .bytes()
                    .await
                    .map_err(|e| Error::Other(e.to_string()))?;
                std::fs::write(&path, bytes)?;
                Ok(Some(path))
            };
            return ActorResponse::r#async(fut.into_actor(self));
        }
    }
}

impl Handler<TransferResource> for TransferService {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: TransferResource, _: &mut Self::Context) -> Self::Result {
        let src_url = actor_try!(TransferUrl::parse(&msg.from, "container"));
        let dst_url = actor_try!(TransferUrl::parse(&msg.to, "container"));
        let src = actor_try!(self.provider(&src_url));
        let dst = actor_try!(self.provider(&dst_url));

        let (abort, reg) = Abort::new_pair();

        let handles = self.abort_handles.clone();
        let fut = async move {
            log::info!("Transferring {:?} to {:?}", src_url.url, dst_url.url);
            {
                let ctx = TransferContext::from(msg.args);
                let retry = transfer_with(src, &src_url, dst, &dst_url, &ctx);

                let _guard = AbortHandleGuard::register(handles, abort);
                Abortable::new(retry, reg)
                    .await
                    .map_err(TransferError::from)??;
            }
            log::info!(
                "Transfer of {:?} to {:?} finished",
                src_url.url,
                dst_url.url
            );
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

impl Handler<AbortTransfers> for TransferService {
    type Result = <AbortTransfers as Message>::Result;

    fn handle(&mut self, _: AbortTransfers, _: &mut Self::Context) -> Self::Result {
        {
            let mut guard = self.abort_handles.borrow_mut();
            std::mem::replace(&mut (*guard), Default::default())
        }
        .into_iter()
        .for_each(|h| h.abort());
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
    inner: Rc<RefCell<HashSet<Abort>>>,
    abort: Abort,
}

impl AbortHandleGuard {
    pub fn register(inner: Rc<RefCell<HashSet<Abort>>>, abort: Abort) -> Self {
        inner.borrow_mut().insert(abort.clone());
        Self { inner, abort }
    }
}

impl Drop for AbortHandleGuard {
    fn drop(&mut self) {
        self.inner.borrow_mut().remove(&self.abort);
    }
}

#[allow(unused)]
async fn move_file(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        #[cfg(target_os = "linux")]
        use std::os::linux::fs::MetadataExt;
        #[cfg(target_os = "macos")]
        use std::os::macos::fs::MetadataExt;

        let src = src.as_ref();
        let dst = dst.as_ref();
        let dst_parent = dst
            .parent()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        let src_meta = src.metadata()?;
        let dst_parent_meta = dst_parent.metadata()?;

        // rename if both are located on the same device, copy & remove otherwise
        if src_meta.st_dev() == dst_parent_meta.st_dev() {
            tokio::fs::rename(src, dst).await
        } else {
            tokio::fs::copy(src, dst).await?;
            tokio::fs::remove_file(src).await
        }
    }

    #[cfg(not(unix))]
    {
        tokio::fs::rename(src, dst).await
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
