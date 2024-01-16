use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use actix::prelude::*;
use futures::future::Abortable;
use futures::{Sink, StreamExt, TryStreamExt};
use url::Url;

use crate::cache::{Cache, CachePath};
use crate::error::Error;
use crate::error::Error as TransferError;
pub use crate::progress::ProgressConfig;
use crate::{
    transfer_with, ContainerTransferProvider, FileTransferProvider, GftpTransferProvider,
    HttpTransferProvider, Retry, TransferContext, TransferData, TransferProvider, TransferUrl,
};

use ya_client_model::activity::exe_script_command::ProgressArgs;
pub use ya_client_model::activity::CommandProgress;
use ya_client_model::activity::TransferArgs;
use ya_runtime_api::deploy::ContainerVolume;
use ya_utils_futures::abort::Abort;

pub type Result<T> = std::result::Result<T, Error>;

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

#[derive(Debug, Message, Default)]
#[rtype(result = "Result<()>")]
pub struct TransferResource {
    pub from: String,
    pub to: String,
    pub args: TransferArgs,
    /// Progress reporting configuration. `None` means that there will be no progress updates.
    pub progress_config: Option<ProgressConfig>,
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct AddVolumes(Vec<ContainerVolume>);

impl AddVolumes {
    pub fn new(vols: Vec<ContainerVolume>) -> Self {
        AddVolumes(vols)
    }
}

#[derive(Debug, Message, Default)]
#[rtype(result = "Result<Option<PathBuf>>")]
pub struct DeployImage {
    pub task_package: Option<String>,
    /// Progress reporting configuration. `None` means that there will be no progress updates.
    pub progress_config: Option<ProgressConfig>,
}

impl DeployImage {
    pub fn with_package(task_package: &str) -> DeployImage {
        DeployImage {
            task_package: Some(task_package.to_string()),
            progress_config: None,
        }
    }
}

pub trait ForwardProgressToSink {
    fn progress_config_mut(&mut self) -> &mut Option<ProgressConfig>;

    fn forward_progress(
        &mut self,
        args: &ProgressArgs,
        sender: impl Sink<CommandProgress, Error = Error> + 'static,
    ) {
        let progress_args = self.progress_config_mut();
        let rx = match progress_args {
            None => {
                let (tx, rx) = tokio::sync::broadcast::channel(50);
                *progress_args = Some(ProgressConfig {
                    progress: tx,
                    progress_args: args.clone(),
                });
                rx
            }
            Some(args) => args.progress.subscribe(),
        };

        tokio::task::spawn_local(async move {
            tokio_stream::wrappers::BroadcastStream::new(rx)
                .map_err(|e| Error::Other(e.to_string()))
                .forward(sender)
                .await
                .ok()
        });
    }
}

impl ForwardProgressToSink for DeployImage {
    fn progress_config_mut(&mut self) -> &mut Option<ProgressConfig> {
        &mut self.progress_config
    }
}

impl ForwardProgressToSink for TransferResource {
    fn progress_config_mut(&mut self) -> &mut Option<ProgressConfig> {
        &mut self.progress_config
    }
}

impl DeployImage {
    pub fn forward_progress(
        &mut self,
        args: &ProgressArgs,
        sender: impl Sink<CommandProgress, Error = Error> + 'static,
    ) {
        let rx = match &self.progress_config {
            None => {
                let (tx, rx) = tokio::sync::broadcast::channel(50);
                self.progress_config = Some(ProgressConfig {
                    progress: tx,
                    progress_args: args.clone(),
                });
                rx
            }
            Some(args) => args.progress.subscribe(),
        };

        tokio::task::spawn_local(async move {
            tokio_stream::wrappers::BroadcastStream::new(rx)
                .map_err(|e| Error::Other(e.to_string()))
                .forward(sender)
                .await
                .ok()
        });
    }
}

#[derive(Clone, Debug, Message)]
#[rtype(result = "()")]
pub struct AbortTransfers;

#[derive(Debug, Default, Message)]
#[rtype(result = "Result<()>")]
pub struct Shutdown;

#[derive(Default)]
pub struct TransferServiceContext {
    pub work_dir: PathBuf,
    pub cache_dir: PathBuf,
    /// TODO: `task_package` should be passed only as `Deploy` message param.
    ///       Problem is that current ExeUnit implementation doesn't have this information
    ///       directly available when sending Deploy, so temporarily we need this ugly solution.   
    pub task_package: Option<String>,

    pub deploy_retry: Option<Retry>,
    pub transfer_retry: Option<Retry>,
}

/// Handles resources transfers.
pub struct TransferService {
    providers: HashMap<&'static str, Rc<dyn TransferProvider<TransferData, TransferError>>>,
    cache: Cache,
    work_dir: PathBuf,
    task_package: Option<String>,

    deploy_retry: Retry,
    transfer_retry: Retry,

    abort_handles: Rc<RefCell<HashSet<Abort>>>,
}

impl TransferService {
    pub fn new(ctx: TransferServiceContext) -> TransferService {
        TransferService {
            providers: Self::default_providers(),
            cache: Cache::new(ctx.cache_dir),
            work_dir: ctx.work_dir,
            task_package: ctx.task_package,
            deploy_retry: ctx.deploy_retry.unwrap_or(Retry::default()),
            transfer_retry: ctx.transfer_retry.unwrap_or(Retry::default()),
            abort_handles: Default::default(),
        }
    }

    pub fn schemes() -> Vec<String> {
        Self::default_providers()
            .values()
            .flat_map(|p| p.schemes())
            .collect::<HashSet<_>>()
            .into_iter()
            .map(ToString::to_string)
            .collect()
    }

    pub fn register_provider(
        &mut self,
        provider: impl TransferProvider<TransferData, TransferError> + 'static,
    ) {
        let provider = Rc::new(provider);
        for scheme in provider.schemes() {
            self.providers.insert(scheme, provider.clone());
        }
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
            .ok_or_else(|| TransferError::UnsupportedSchemeError(scheme.to_owned()))?
            .clone())
    }

    #[cfg(feature = "sgx")]
    fn deploy_sgx(
        &self,
        src_url: TransferUrl,
        _src_name: CachePath,
        path: PathBuf,
        ctx: TransferContext,
    ) -> ActorResponse<Self, Result<Option<PathBuf>>> {
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
        ActorResponse::r#async(fut.into_actor(self))
    }

    #[allow(unused)]
    fn deploy_no_sgx(
        &self,
        src_url: TransferUrl,
        src_name: CachePath,
        path: PathBuf,
        ctx: TransferContext,
    ) -> ActorResponse<Self, Result<Option<PathBuf>>> {
        let path_tmp = self.cache.to_temp_path(&src_name).to_path_buf();

        let src = actor_try!(self.provider(&src_url));
        let dst: Rc<FileTransferProvider> = Default::default();
        let dst_url = TransferUrl {
            url: Url::from_file_path(&path_tmp).unwrap(),
            hash: None,
        };

        // Using partially downloaded image from previous executions could speed up deploy
        // process, but it comes with the cost: If image under URL changed, Requestor will get
        // error on the end. This can result with Provider being perceived as unreliable.
        //
        // For this reason it is better to use only fully downloaded images that are already in cache.
        if path_tmp.exists() {
            log::info!(
                "Removing temporary file: {} from previous executions",
                path_tmp.display()
            );
            std::fs::remove_file(&path_tmp).ok();
        }

        let handles = self.abort_handles.clone();
        let fut = async move {
            if path.exists() {
                log::info!("Deploying cached image: {:?}", path);
                ctx.reporter()
                    .report_message("Deployed image from cache".to_string());
                return Ok(Some(path));
            }

            let (abort, reg) = Abort::new_pair();
            {
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
        ActorResponse::r#async(fut.into_actor(self))
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

impl Handler<DeployImage> for TransferService {
    type Result = ActorResponse<Self, Result<Option<PathBuf>>>;

    #[allow(unused_variables)]
    fn handle(&mut self, deploy: DeployImage, ctx: &mut Self::Context) -> Self::Result {
        let image = match deploy.task_package.or(self.task_package.clone()) {
            Some(image) => image,
            None => return ActorResponse::reply(Ok(None)),
        };

        let src_url = actor_try!(TransferUrl::parse_with_hash(&image, "file"));
        let src_name = actor_try!(Cache::name(&src_url));
        let path = self.cache.to_final_path(&src_name).to_path_buf();

        log::info!("Deploying from {:?} to {:?}", src_url.url, path);

        let mut ctx = TransferContext::default();
        ctx.state.retry_with(self.deploy_retry.clone());
        ctx.progress
            .register_reporter(deploy.progress_config, 1, Some("Bytes".to_string()));

        #[cfg(not(feature = "sgx"))]
        return self.deploy_no_sgx(src_url, src_name, path, ctx);

        #[cfg(feature = "sgx")]
        return self.deploy_sgx(src_url, src_name, path, ctx);
    }
}

impl Handler<TransferResource> for TransferService {
    type Result = ActorResponse<Self, Result<()>>;

    fn handle(&mut self, msg: TransferResource, _: &mut Self::Context) -> Self::Result {
        let src_url = actor_try!(TransferUrl::parse(&msg.from, "container"));
        let dst_url = actor_try!(TransferUrl::parse(&msg.to, "container"));
        let src = actor_try!(self.provider(&src_url));
        let dst = actor_try!(self.provider(&dst_url));

        let mut ctx = TransferContext::default();
        ctx.state.retry_with(self.transfer_retry.clone());
        ctx.progress
            .register_reporter(msg.progress_config, 1, Some("Bytes".to_string()));

        let (abort, reg) = Abort::new_pair();

        let handles = self.abort_handles.clone();
        let fut = async move {
            log::info!("Transferring {:?} to {:?}", src_url.url, dst_url.url);
            {
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
            std::mem::take(&mut (*guard))
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
