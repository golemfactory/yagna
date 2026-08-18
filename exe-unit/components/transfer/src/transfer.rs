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
use crate::file::GuardedFileTransferProvider;
use crate::hash::verify_file_hash;
use crate::location::TransferHash;
pub use crate::progress::ProgressConfig;
use crate::{
    transfer_with, ContainerTransferProvider, GftpTransferProvider, HttpTransferProvider, Retry,
    TransferContext, TransferData, TransferProvider, TransferUrl,
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
            deploy_retry: ctx.deploy_retry.unwrap_or_default(),
            transfer_retry: ctx.transfer_retry.unwrap_or_default(),
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
        src_name: CachePath,
        path: PathBuf,
        _ctx: TransferContext,
    ) -> ActorResponse<Self, Result<Option<PathBuf>>> {
        let cache = self.cache.clone();
        let expected_hash = actor_try!(src_url
            .hash
            .clone()
            .ok_or_else(|| TransferError::InvalidUrlError("hash required".to_owned())));
        let fut = async move {
            let _lock = cache.lock(&src_name).await?;
            if trusted_cache_entry_exists(&path).await? {
                log::info!("Using trusted deploy image cache entry");
                return Ok(Some(path));
            }

            let partial_path = cache.to_partial_path(&src_name).to_path_buf();
            let mut resp = crate::sandboxed_http::SandboxedHttpClient::from_env()
                .get(src_url.url)
                .await?;
            let bytes = resp.body().limit(usize::MAX).await.map_err(Error::from)?;
            tokio::fs::write(&partial_path, bytes).await?;
            sync_file(&partial_path).await?;
            verify_and_publish(&partial_path, &path, &expected_hash).await?;
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
        let src = actor_try!(self.provider(&src_url));
        let expected_hash = actor_try!(src_url
            .hash
            .clone()
            .ok_or_else(|| TransferError::InvalidUrlError("hash required".to_owned())));
        let cache = self.cache.clone();
        let handles = self.abort_handles.clone();
        let fut = async move {
            let lock = Rc::new(cache.lock(&src_name).await?);
            if trusted_cache_entry_exists(&path).await? {
                log::info!("Using trusted deploy image cache entry");
                ctx.reporter()
                    .report_message("Deployed image from cache".to_string());
                return Ok(Some(path));
            }

            let path_tmp = cache.to_partial_path(&src_name).to_path_buf();
            if let Ok(metadata) = tokio::fs::metadata(&path_tmp).await {
                if metadata.len() > 0 {
                    log::info!(
                        "Resuming deploy image download from offset {}",
                        metadata.len()
                    );
                }
            }
            let dst_url = TransferUrl {
                url: Url::from_file_path(&path_tmp).map_err(|_| {
                    TransferError::InvalidUrlError(format!(
                        "Invalid staging path: {}",
                        path_tmp.display()
                    ))
                })?,
                hash: None,
            };

            // Stream hashing is deliberately disabled for deploy images. The completed on-disk
            // partial is hashed exactly once after fsync, immediately before publication.
            let mut download_url = src_url.clone();
            download_url.hash = None;
            // The destination task can briefly outlive cancellation of `transfer_with`. Keeping
            // the lock in that task prevents a waiter from opening the same partial until the old
            // writer has flushed, synced, and closed it.
            let dst = Rc::new(GuardedFileTransferProvider::new(lock.clone()));
            let (abort, reg) = Abort::new_pair();
            {
                let retry = transfer_with(src, &download_url, dst, &dst_url, &ctx);

                let _guard = AbortHandleGuard::register(handles, abort);
                Abortable::new(retry, reg)
                    .await
                    .map_err(TransferError::from)??;
            }

            sync_file(&path_tmp).await?;
            verify_and_publish(&path_tmp, &path, &expected_hash).await?;
            log::info!("Deployment from {:?} finished", src_url.url);

            Ok(Some(path))
        };
        ActorResponse::r#async(fut.into_actor(self))
    }
}

async fn trusted_cache_entry_exists(path: &Path) -> Result<bool> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

async fn sync_file(path: &Path) -> Result<()> {
    tokio::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .await?
        .sync_all()
        .await?;
    Ok(())
}

async fn verify_and_publish(
    partial_path: &Path,
    final_path: &Path,
    expected_hash: &TransferHash,
) -> Result<()> {
    if let Err(error) = verify_file_hash(partial_path, expected_hash).await {
        if matches!(error, TransferError::InvalidHashError { .. }) {
            if let Err(remove_error) = tokio::fs::remove_file(partial_path).await {
                if remove_error.kind() != std::io::ErrorKind::NotFound {
                    return Err(remove_error.into());
                }
            }
        }
        return Err(error);
    }

    match tokio::fs::hard_link(partial_path, final_path).await {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Only this versioned namespace is trusted. Every candidate is verified before this
            // point, so a concurrent winner can be reused without another full hash read.
            if !trusted_cache_entry_exists(final_path).await? {
                return Err(error.into());
            }
        }
        Err(error) => return Err(error.into()),
    }

    if let Err(error) = tokio::fs::remove_file(partial_path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "Unable to remove verified deploy image partial after publication: {}",
                error
            );
        }
    }
    Ok(())
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

        let mut ctx = TransferContext::from(msg.args);
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
        std::fs::create_dir_all(&self.work_dir)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use sha3::{Digest, Sha3_256};

    fn sha3_256(data: &[u8]) -> TransferHash {
        TransferHash {
            alg: "sha3".to_string(),
            val: Sha3_256::digest(data).to_vec(),
        }
    }

    #[actix_rt::test]
    async fn hash_mismatch_is_not_published_and_removes_partial() {
        let dir = tempdir::TempDir::new("corrupted-partial").unwrap();
        let partial = dir.path().join("partial");
        let final_path = dir.path().join("final");
        tokio::fs::write(&partial, b"corrupted").await.unwrap();

        let error = verify_and_publish(&partial, &final_path, &sha3_256(b"expected"))
            .await
            .unwrap_err();

        assert!(matches!(error, TransferError::InvalidHashError { .. }));
        assert!(!final_path.exists());
        assert!(!partial.exists());
    }

    #[actix_rt::test]
    async fn concurrent_publications_converge_on_verified_entry() {
        let dir = tempdir::TempDir::new("concurrent-publication").unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        let final_path = dir.path().join("final");
        let contents = b"verified image contents";
        let expected_hash = sha3_256(contents);
        tokio::fs::write(&first, contents).await.unwrap();
        tokio::fs::write(&second, contents).await.unwrap();

        let (first_result, second_result) = futures::join!(
            verify_and_publish(&first, &final_path, &expected_hash),
            verify_and_publish(&second, &final_path, &expected_hash),
        );

        first_result.unwrap();
        second_result.unwrap();
        assert_eq!(tokio::fs::read(&final_path).await.unwrap(), contents);
        verify_file_hash(&final_path, &expected_hash).await.unwrap();
        assert!(!first.exists());
        assert!(!second.exists());
    }

    #[actix_rt::test]
    async fn trusted_cache_hit_is_metadata_only_and_does_not_recompute_hash() {
        let dir = tempdir::TempDir::new("trusted-cache-hit").unwrap();
        let final_path = dir.path().join("final");
        // These contents intentionally do not match any expected digest. A trusted namespace hit
        // is decided only by entry existence; the full-file verifier is reserved for candidates
        // immediately before their first publication.
        tokio::fs::write(&final_path, b"not re-hashed on cache hit")
            .await
            .unwrap();

        let exists = trusted_cache_entry_exists(&final_path).await.unwrap();

        assert!(exists);
    }
}
