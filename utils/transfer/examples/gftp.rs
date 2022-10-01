use actix_rt::System;
use futures::channel::oneshot;
use gftp::open_for_upload;
use rand::Rng;
use sha3::digest::generic_array::GenericArray;
use sha3::Digest;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::{env, thread};
use tempdir::TempDir;
use url::Url;
use ya_transfer::error::Error;
use ya_transfer::{
    transfer, FileTransferProvider, GftpTransferProvider, TransferContext, TransferProvider,
};

type HashOutput = GenericArray<u8, <sha3::Sha3_512 as Digest>::OutputSize>;

fn create_file(path: &Path, name: &str, chunk_size: usize, chunk_count: usize) -> HashOutput {
    let path = path.join(name);
    let mut hasher = sha3::Sha3_512::default();
    let mut file_src = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .expect("rnd file");

    let mut rng = rand::thread_rng();

    for _ in 0..chunk_count {
        let input: Vec<u8> = (0..chunk_size).map(|_| rng.gen_range(0..=255u8)).collect();

        hasher.input(&input);
        let _ = file_src.write(&input).unwrap();
    }
    file_src.flush().unwrap();
    hasher.result()
}

fn hash_file(path: &Path) -> HashOutput {
    let mut file_src = OpenOptions::new().read(true).open(path).expect("rnd file");

    let mut hasher = sha3::Sha3_512::default();
    let mut chunk = vec![0; 4096];

    while let Ok(count) = file_src.read(&mut chunk[..]) {
        hasher.input(&chunk[..count]);
        if count != 4096 {
            break;
        }
    }
    hasher.result()
}

#[actix_rt::main]
async fn main() -> Result<(), Error> {
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
    );
    env_logger::init();

    let temp_dir = TempDir::new("transfer").unwrap();
    let chunk_size = 4096 as usize;
    let chunk_count = 256 as usize;

    log::info!(
        "Creating a random file of size {} * {}",
        chunk_size,
        chunk_count
    );

    let hash = create_file(temp_dir.path(), "rnd", chunk_size, chunk_count);
    let path = temp_dir.path().join("rnd");
    let path_dl = temp_dir.path().join("rnd2");
    let path_up = temp_dir.path().join("rnd3");

    let gftp_provider = GftpTransferProvider::default();
    let file_provider = FileTransferProvider::default();
    let (tx, rx) = oneshot::channel();

    let path_th = path.clone();
    thread::spawn(move || {
        System::new().block_on(async move {
            let url = gftp::publish(&path_th).await.unwrap();
            log::info!("Publishing file at {:?}", url);
            tx.send(url).unwrap();
            actix_rt::signal::ctrl_c().await.unwrap();
        })
    });

    let src_url = rx.await.unwrap();
    let dest_url = Url::parse(&format!("file://{}", path_dl.to_str().unwrap()))?;

    let ctx = TransferContext::default();
    let dest = file_provider.destination(&dest_url, &ctx);
    let source = gftp_provider.source(&src_url, &ctx);

    log::info!("Sharing file at {:?}", src_url.path());
    log::info!("Expecting file at {:?}", dest_url.path());

    transfer(source, dest).await?;

    log::info!(
        "Transfer complete, comparing hashes of {:?} vs {:?}",
        &path,
        &path_dl
    );
    assert_eq!(hash, hash_file(&path_dl));

    let (tx, rx) = oneshot::channel();

    let path_th = path_up.clone();
    thread::spawn(move || {
        System::new().block_on(async move {
            let url = open_for_upload(&path_th).await.unwrap();
            log::info!("Awaiting upload at {:?}", url);
            tx.send(url).unwrap();
            actix_rt::signal::ctrl_c().await.unwrap();
        })
    });

    let src_url = Url::parse(&format!("file://{}", path_dl.to_str().unwrap()))?;
    let dest_url = rx.await.unwrap();

    let ctx = TransferContext::default();
    let dest = gftp_provider.destination(&dest_url, &ctx);
    let source = file_provider.source(&src_url, &ctx);

    log::info!("Sharing file at {:?}", src_url.path());
    log::info!("Expecting file at {:?}", dest_url.path());

    transfer(source, dest).await?;

    log::info!(
        "Transfer complete, comparing hashes of {:?} vs {:?}",
        &path_dl,
        &path_up
    );
    assert_eq!(hash, hash_file(&path_up));

    Ok(())
}
