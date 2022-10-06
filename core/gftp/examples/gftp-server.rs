use anyhow::{anyhow, Result};
use futures::future::{FutureExt, LocalBoxFuture};
use gftp::rpc::*;
use sha3::digest::generic_array::GenericArray;
use sha3::Digest;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use structopt::StructOpt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};

static SEQ: AtomicUsize = AtomicUsize::new(0);
type HashOutput = GenericArray<u8, <sha3::Sha3_512 as Digest>::OutputSize>;

/// Build the GFTP binary, start the daemon and run:
///
/// `cargo run --example gftp-server ../../target/debug/gftp Cargo.toml`
#[derive(StructOpt)]
struct Args {
    /// Path to GFTP binary
    gftp_bin: PathBuf,
    /// File to share
    share: PathBuf,
}

trait ReadRpcMessage {
    fn read_message(&mut self) -> LocalBoxFuture<Result<RpcMessage>>;
}

trait WriteRpcMessage {
    fn write_message(&mut self, msg: RpcMessage) -> LocalBoxFuture<Result<()>>;
}

impl ReadRpcMessage for BufReader<ChildStdout> {
    fn read_message(&mut self) -> LocalBoxFuture<Result<RpcMessage>> {
        async move {
            let mut buffer = String::new();
            self.read_line(&mut buffer).await?;
            log::info!("[Rx] {}", buffer.trim());
            let msg = serde_json::from_str::<RpcMessage>(&buffer)?;
            Ok(msg)
        }
        .boxed_local()
    }
}

impl WriteRpcMessage for ChildStdin {
    fn write_message(&mut self, msg: RpcMessage) -> LocalBoxFuture<Result<()>> {
        async move {
            let ser = format!("{}\r\n", serde_json::to_string(&msg)?);
            log::info!("[Tx] {}", ser.trim());
            self.write_all(ser.as_bytes()).await?;
            self.flush().await?;
            Ok(())
        }
        .boxed_local()
    }
}

async fn send(
    stdin: &mut ChildStdin,
    reader: &mut BufReader<ChildStdout>,
    req: RpcRequest,
) -> Result<RpcResult> {
    let id = SEQ.fetch_add(1, Ordering::Relaxed) as i64;
    let msg = RpcMessage::request(Some(&RpcId::Int(id)), req);
    stdin.write_message(msg).await?;

    let res = reader.read_message().await?;
    match res.id {
        Some(RpcId::Int(v)) => {
            if v != id {
                return Err(anyhow!("Invalid response ID: {}, expected {}", v, id));
            }
        }
        id => return Err(anyhow!("Invalid response ID: {:?}", id)),
    }

    match res.body {
        RpcBody::Error { error } => Err(anyhow!("Request {:?} failed: {:?}", id, error)),
        RpcBody::Request { .. } => Err(anyhow!("Unexpected message: {:?}", res)),
        RpcBody::Result { result } => Ok(result),
    }
}

fn hash_file(path: &Path) -> Result<HashOutput> {
    let mut file_src = OpenOptions::new().read(true).open(path)?;

    let mut hasher = sha3::Sha3_512::default();
    let mut chunk = vec![0; 4096];

    while let Ok(count) = file_src.read(&mut chunk[..]) {
        hasher.input(&chunk[..count]);
        if count != 4096 {
            break;
        }
    }
    Ok(hasher.result())
}

#[actix_rt::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    std::env::set_var(
        "RUST_LOG",
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
    );
    env_logger::init();

    let args = Args::from_args();
    if !args.gftp_bin.exists() {
        return Err(anyhow!(
            "Gftp binary does not exist: {}",
            args.gftp_bin.display()
        ));
    }
    if !args.share.exists() {
        return Err(anyhow!(
            "Shared file does not exist: {}",
            args.gftp_bin.display()
        ));
    }

    let tmp_dir = tempdir::TempDir::new("gftp-server")?;
    let published_hash = hash_file(&args.share)?;

    log::info!("spawning server");
    let mut child = Command::new(args.gftp_bin)
        .arg(OsString::from("server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    log::info!("sending version request");
    let req = RpcRequest::Version {};
    send(&mut stdin, &mut reader, req).await?;

    log::info!("sending publish request");
    let files = vec![args.share.clone()];
    let req = RpcRequest::Publish { files };
    let urls = match send(&mut stdin, &mut reader, req).await? {
        RpcResult::Files(files) => files.into_iter().map(|r| r.url).collect::<Vec<_>>(),
        result => return Err(anyhow!("Invalid result: {:?}", result)),
    };

    log::info!("sending close request");
    let req = RpcRequest::Close { urls: urls.clone() };
    match send(&mut stdin, &mut reader, req).await? {
        RpcResult::Statuses(vec) => {
            if vec.iter().any(|b| b == &RpcStatusResult::Error) {
                return Err(anyhow!("Invalid result: {:?}", vec));
            }
        }
        result => return Err(anyhow!("Invalid result: {:?}", result)),
    }

    log::info!("sending erroneous close request");
    let req = RpcRequest::Close { urls };
    match send(&mut stdin, &mut reader, req).await? {
        RpcResult::Statuses(vec) => {
            if vec.iter().any(|b| b == &RpcStatusResult::Ok) {
                return Err(anyhow!("Invalid result: {:?}", vec));
            }
        }
        result => return Err(anyhow!("Invalid result: {:?}", result)),
    }

    log::info!("sending publish request (for download)");
    let files = vec![args.share.clone()];
    let req = RpcRequest::Publish { files };
    let url = match send(&mut stdin, &mut reader, req).await? {
        RpcResult::Files(files) => files
            .into_iter()
            .map(|r| r.url)
            .next()
            .ok_or_else(|| anyhow!("Missing URL in response"))?,
        result => return Err(anyhow!("Invalid result: {:?}", result)),
    };

    log::info!("sending download request");
    let output_file = tmp_dir.path().join("tmp-download");
    let req = RpcRequest::Download {
        url,
        output_file: output_file.clone(),
    };
    send(&mut stdin, &mut reader, req).await?;

    if hash_file(&output_file)? != published_hash {
        return Err(anyhow!("Invalid file hash (receive request)"));
    } else {
        log::info!("file checksum ok");
    }

    log::info!("sending receive request");
    let output_file = tmp_dir.path().join("tmp-receive");
    let req = RpcRequest::Receive {
        output_file: output_file.clone(),
    };
    let url = match send(&mut stdin, &mut reader, req).await? {
        RpcResult::File(file_result) => file_result.url,
        result => return Err(anyhow!("Invalid result: {:?}", result)),
    };

    log::info!("sending upload request");
    let req = RpcRequest::Upload {
        url,
        file: args.share,
    };
    send(&mut stdin, &mut reader, req).await?;

    if hash_file(&output_file)? != published_hash {
        return Err(anyhow!("Invalid file hash (receive request)"));
    } else {
        log::info!("file checksum ok");
    }

    log::info!("sending shutdown request");
    let req = RpcRequest::Shutdown {};
    send(&mut stdin, &mut reader, req).await?;

    child.wait().await?;
    Ok(())
}
