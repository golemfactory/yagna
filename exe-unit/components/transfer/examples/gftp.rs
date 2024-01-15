use crossterm::{cursor, terminal, ExecutableCommand, QueueableCommand};
use rand::RngCore;
use sha3::digest::generic_array::GenericArray;
use sha3::Digest;
use std::env;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;
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
    let mut input: Vec<u8> = vec![0; chunk_size];

    for _ in 0..chunk_count {
        rng.fill_bytes(&mut input);

        hasher.input(&input);
        file_src.write_all(&input).unwrap();
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

// processing progress updates must not panic or the transfer will be aborted
fn progress_to_stdout(start_offset: u64, start_time: Instant, progress: u64, size: Option<u64>) {
    let elapsed = start_time.elapsed();
    let elapsed_secs = elapsed.as_secs() as f64 + elapsed.subsec_nanos() as f64 * 1e-9;
    let (speed, unit) = {
        let raw_speed = (progress - start_offset) as f64 / elapsed_secs;
        if raw_speed > 1024.0 * 1024.0 {
            (raw_speed / 1024.0 / 1024.0, "MB/s")
        } else if raw_speed > 1024.0 {
            (raw_speed / 1024.0, "KB/s")
        } else {
            (raw_speed, "B/s")
        }
    };
    let (percent, total_size) = if let Some(total_bytes) = size {
        (
            format!("{:.2}", 100.0 * progress as f64 / total_bytes as f64),
            total_bytes.to_string(),
        )
    } else {
        ("--".into(), "unknown".into())
    };

    let mut stdout = std::io::stdout();
    stdout
        .queue(terminal::Clear(terminal::ClearType::CurrentLine))
        .ok();
    stdout
        .write_all(
            format!(
                "{} / {} ({:.2} {}) {}%",
                progress, total_size, speed, unit, percent
            )
            .as_bytes(),
        )
        .ok();
    stdout.queue(cursor::RestorePosition).ok();
    stdout.flush().ok();
}

#[actix_rt::main]
async fn main() -> Result<(), Error> {
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
    );
    env_logger::init();

    let temp_dir = TempDir::new("transfer").unwrap();
    let chunk_size = 4096_usize;
    let chunk_count = 25600_usize;

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
    let file_provider = FileTransferProvider;

    let src_url = gftp::publish(&path).await.unwrap();
    let dest_url = Url::parse(&format!("file://{}", path_dl.to_str().unwrap()))?;
    log::info!("Publishing file at {}", src_url);
    log::info!("Sharing file at {:?}", src_url.path());
    log::info!("Expecting file at {:?}", dest_url.path());

    let ctx = TransferContext::default();
    let source = gftp_provider.source(&src_url, &ctx);
    let dest = file_provider.destination(&dest_url, &ctx);

    // progress reporting on the TransferSink

    let mut stdout = std::io::stdout();
    stdout.execute(cursor::Hide).ok();
    stdout.queue(cursor::SavePosition).ok();
    let start_offset = ctx.state.offset();
    let start_time = Instant::now();
    let dest_with_progress =
        ya_transfer::wrap_sink_with_progress_reporting(dest, &ctx, move |progress, size| {
            progress_to_stdout(start_offset, start_time, progress, size);
            // could also log progress:
            // log::info!("Transfer progress {} / {} ({:.2}%)", progress, size, 100.0 * progress as f64 / size as f64);
        });

    let transfer_done = transfer(source, dest_with_progress).await;
    stdout.execute(cursor::Show).ok();
    writeln!(stdout).ok();
    transfer_done?;

    log::info!(
        "Transfer complete, comparing hashes of {:?} vs {:?}",
        &path,
        &path_dl
    );
    assert_eq!(hash, hash_file(&path_dl));

    let src_url = Url::parse(&format!("file://{}", path_dl.to_str().unwrap()))?;
    let dest_url = gftp::open_for_upload(&path_up).await.unwrap();

    log::info!("Awaiting upload at {}", dest_url);
    log::info!("Sharing file at {:?}", src_url.path());
    log::info!("Expecting file at {:?}", dest_url.path());

    let ctx = TransferContext::default();
    let source = file_provider.source(&src_url, &ctx);

    // works on TransferStreams also

    let mut stdout = std::io::stdout();
    stdout.execute(cursor::Hide).ok();
    stdout.queue(cursor::SavePosition).ok();
    let start_offset = ctx.state.offset();
    let start_time = Instant::now();
    let source_with_progress =
        ya_transfer::wrap_stream_with_progress_reporting(source, &ctx, move |progress, size| {
            progress_to_stdout(start_offset, start_time, progress, size);
        });

    let dest = gftp_provider.destination(&dest_url, &ctx);

    let transfer_done = transfer(source_with_progress, dest).await;
    stdout.execute(cursor::Show).ok();
    writeln!(stdout).ok();
    transfer_done?;

    log::info!(
        "Transfer complete, comparing hashes of {:?} vs {:?}",
        &path_dl,
        &path_up
    );
    assert_eq!(hash, hash_file(&path_up));

    Ok(())
}
