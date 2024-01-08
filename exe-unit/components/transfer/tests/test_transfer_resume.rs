use actix::{Actor, Addr};
use futures::future::LocalBoxFuture;
use std::io::ErrorKind;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, io};
use test_context::test_context;
use url::Url;

use ya_client_model::activity::TransferArgs;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::file::generate_file_with_hash;
use ya_framework_basic::hash::verify_hash;
use ya_framework_basic::log::enable_logs;
use ya_framework_basic::server_external::start_http;
use ya_framework_basic::temp_dir;
use ya_runtime_api::deploy::ContainerVolume;
use ya_transfer::error::{Error, HttpError};
use ya_transfer::transfer::{
    AddVolumes, TransferResource, TransferService, TransferServiceContext,
};
use ya_transfer::*;

struct UnreliableHttpProvider {
    inner: HttpTransferProvider,
    last_failure: Arc<Mutex<Instant>>,
    interval: Duration,
    packets_slowdown: Duration,
}

impl UnreliableHttpProvider {
    pub fn new(interval: u64) -> Self {
        Self {
            inner: Default::default(),
            last_failure: Arc::new(Mutex::new(Instant::now())),
            interval: Duration::from_millis(interval),
            packets_slowdown: Duration::from_millis(10),
        }
    }
}

impl TransferProvider<TransferData, Error> for UnreliableHttpProvider {
    fn schemes(&self) -> Vec<&'static str> {
        self.inner.schemes()
    }

    fn source(&self, url: &Url, ctx: &TransferContext) -> TransferStream<TransferData, Error> {
        let mut src = self.inner.source(url, ctx);
        let interval = self.interval;
        let failure = self.last_failure.clone();
        let slowdown = self.packets_slowdown;

        // Slow down stream
        src.map_inner_async(move |item| {
            let slowdown = slowdown;
            Box::pin(async move {
                tokio::time::sleep(slowdown).await;
                item
            })
        });

        src.map_inner(move |r| match r {
            Ok(v) => {
                log::trace!("Processing packet of size: {}", v.as_ref().len());

                let instant = { *failure.lock().unwrap() };
                if Instant::now() - instant >= interval {
                    log::info!("Triggering failure");

                    let mut guard = failure.lock().unwrap();
                    *guard = Instant::now();

                    Err(HttpError::Io(io::Error::from(ErrorKind::Interrupted)).into())
                } else {
                    Ok(v)
                }
            }
            Err(e) => Err(e),
        });
        src
    }

    fn destination(&self, url: &Url, ctx: &TransferContext) -> TransferSink<TransferData, Error> {
        self.inner.destination(url, ctx)
    }

    fn prepare_source<'a>(
        &self,
        url: &Url,
        ctx: &TransferContext,
    ) -> LocalBoxFuture<'a, Result<(), Error>> {
        self.inner.prepare_source(url, ctx)
    }

    fn prepare_destination<'a>(
        &self,
        url: &Url,
        ctx: &TransferContext,
    ) -> LocalBoxFuture<'a, Result<(), Error>> {
        self.inner.prepare_destination(url, ctx)
    }
}

async fn transfer_with_args(
    addr: &Addr<TransferService>,
    from: &str,
    to: &str,
    args: TransferArgs,
) -> Result<(), ya_exe_unit::error::Error> {
    log::info!("Triggering transfer from {} to {}", from, to);

    addr.send(TransferResource {
        from: from.to_owned(),
        to: to.to_owned(),
        args,
        progress_args: None,
    })
    .await??;

    Ok(())
}

async fn transfer(
    addr: &Addr<TransferService>,
    from: &str,
    to: &str,
) -> Result<(), ya_exe_unit::error::Error> {
    transfer_with_args(addr, from, to, TransferArgs::default()).await
}

#[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
async fn test_transfer_resume(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(false);

    let dir = temp_dir!("transfer-resume")?;
    let temp_dir = dir.path();

    log::debug!("Creating directories in: {}", temp_dir.display());
    let work_dir = temp_dir.join("work_dir");

    log::debug!("Starting TransferService");
    let mut retry = Retry::new(i32::MAX);
    retry.backoff(1., 1.);
    let exe_ctx = TransferServiceContext {
        work_dir: work_dir.clone(),
        cache_dir: temp_dir.join("cache_dir"),
        transfer_retry: Some(retry),
        ..TransferServiceContext::default()
    };

    let interval: u64 = 2000;
    let mut service = TransferService::new(exe_ctx);
    service.register_provider(UnreliableHttpProvider::new(interval));

    let addr = service.start();

    let volumes = vec![ContainerVolume {
        name: "vol-1".into(),
        path: "/input".into(),
    }];
    addr.send(AddVolumes::new(volumes)).await??;

    let hash = generate_file_with_hash(temp_dir, "rnd", 4096_usize, 3 * 1024);

    log::debug!("Starting HTTP servers");
    start_http(ctx, temp_dir.to_path_buf())
        .await
        .expect("unable to start http servers");

    log::warn!("[>>] Transfer HTTP -> container");
    tokio::time::timeout(
        Duration::from_secs(20),
        transfer(&addr, "http://127.0.0.1:8001/rnd", "container:/input/rnd-1"),
    )
    .await??;
    verify_hash(&hash, work_dir.join("vol-1"), "rnd-1");
    log::warn!("Checksum verified");

    Ok(())
}
