use futures::channel::mpsc::channel;
use futures::StreamExt;
use std::convert::TryFrom;
use std::env;
use std::path::PathBuf;
use structopt::StructOpt;
use url::Url;
use ya_client_model::activity::{FileSet, SetEntry, TransferArgs};
use ya_transfer::{archive, extract};
use ya_transfer::{ArchiveFormat, HttpTransferProvider, PathTraverse, TransferProvider};

#[derive(StructOpt)]
struct Args {
    url: Url,
    glob: String,
    src_path: PathBuf,
    dst_path: PathBuf,
    #[structopt(long, default_value = "tar.gz")]
    format: String,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();

    let args = Args::from_args();
    let http_provider = HttpTransferProvider::default();
    let format: ArchiveFormat = args.url.path().parse()?;

    let (tx, rx) = channel(1);
    tokio::task::spawn_local(async move {
        rx.for_each(|evt| {
            log::info!("Extract: {:?}", evt);
            futures::future::ready(())
        })
        .await;
    });

    log::warn!("Extracting {:?} to {:?}", args.url, args.src_path);
    let stream = http_provider.source(&args.url, &TransferArgs::default());
    extract(stream, args.src_path.clone(), format, tx).await?;

    log::warn!("Starting on-the-fly compression & extraction");
    let fileset = FileSet::Pattern(SetEntry::Single(args.glob.clone()));
    let mut transfer_args = TransferArgs::default();
    transfer_args.fileset = Some(fileset);

    log::warn!("Transfer args: {:?}", transfer_args);
    let (c_tx, c_rx) = channel(1);
    let (e_tx, e_rx) = channel(1);

    tokio::task::spawn_local(async move {
        c_rx.for_each(|evt| {
            log::info!("Compress: {:?}", evt);
            futures::future::ready(())
        })
        .await;
    });
    tokio::task::spawn_local(async move {
        e_rx.for_each(|evt| {
            log::info!("Extract: {:?}", evt);
            futures::future::ready(())
        })
        .await;
    });

    log::warn!(
        "Compressing {:?} (glob: {:?}), decompressing to {:?}",
        args.src_path,
        args.glob,
        args.dst_path
    );

    let format = ArchiveFormat::try_from(args.format.as_str())?;
    let path_it = transfer_args.traverse(&args.src_path)?;

    let stream = archive(path_it, args.src_path.clone(), format, c_tx).await;
    extract(stream, args.dst_path.clone(), format, e_tx).await?;

    Ok(())
}
