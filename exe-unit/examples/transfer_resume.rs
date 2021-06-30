use std::env;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::future::LocalBoxFuture;
use sha3::digest::generic_array::GenericArray;
use sha3::Digest;
use structopt::StructOpt;
use tempdir::TempDir;
use url::Url;

use std::rc::Rc;
use ya_transfer::error::{Error, HttpError};
use ya_transfer::*;

#[derive(StructOpt, Debug)]
pub struct Cli {
    /// HTTP resource URL
    url: String,
    /// HTTP resource hash
    hash: String,
    /// Forced failure interval in ms
    #[structopt(short, long, default_value = "5000")]
    interval: u64,
}

struct UnreliableHttpProvider {
    inner: HttpTransferProvider,
    last_failure: Arc<Mutex<Instant>>,
    interval: Duration,
}

impl UnreliableHttpProvider {
    pub fn new(interval: u64) -> Self {
        Self {
            inner: Default::default(),
            last_failure: Arc::new(Mutex::new(Instant::now())),
            interval: Duration::from_millis(interval),
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

        src.map_inner(move |r| match r {
            Ok(v) => {
                let instant = { *failure.lock().unwrap() };
                if Instant::now() - instant >= interval {
                    log::info!("Triggering failure");

                    let mut guard = failure.lock().unwrap();
                    *guard = Instant::now();

                    Err(HttpError::Io(ErrorKind::Interrupted).into())
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

fn hash_file<P: AsRef<Path>>(path: P) -> GenericArray<u8, <sha3::Sha3_224 as Digest>::OutputSize> {
    const CHUNK_SIZE: usize = 40960;

    let mut file_src = OpenOptions::new().read(true).open(path).expect("rnd file");
    let mut hasher = sha3::Sha3_224::default();
    let mut chunk = vec![0; CHUNK_SIZE];

    while let Ok(count) = file_src.read(&mut chunk[..]) {
        hasher.input(&chunk[..count]);
        if count != CHUNK_SIZE {
            break;
        }
    }
    hasher.result()
}

async fn download<P: AsRef<Path>>(dst_path: P, args: Cli) -> anyhow::Result<()> {
    let dst_path = dst_path.as_ref();

    let src_url = TransferUrl::parse(&args.url, "http")?;
    let dst_url = TransferUrl::parse(&dst_path.display().to_string(), "file")?;
    let src = Rc::new(UnreliableHttpProvider::new(args.interval));
    let dst = Rc::new(FileTransferProvider::default());
    let ctx = TransferContext::default();

    let mut retry = Retry::new(i32::MAX);
    retry.backoff(1., 1.);
    ctx.state.retry_with(retry);

    transfer_with(&src, &src_url, &dst, &dst_url, &ctx).await?;

    log::info!("File downloaded, verifying contents");

    let hash = hex::encode(hash_file(&dst_path));

    log::info!("input  hash: {}", args.hash);
    log::info!("result hash: {}", hash);

    if args.hash != hash {
        anyhow::bail!("hash mismatch");
    }
    Ok(())
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or(
            "debug,\
            h2=info,\
            trust_dns_proto=info,\
            trust_dns_resolver=info,\
            "
            .into(),
        ),
    );
    env_logger::init();

    let args: Cli = Cli::from_args();
    let dir = TempDir::new("transfer-resume")?;
    let dst_path = dir.path().join("downloaded");

    download(&dst_path, args).await.map_err(|e| {
        let _ = std::fs::remove_file(&dst_path);
        e
    })?;

    Ok(())
}
