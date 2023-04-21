use anyhow::{anyhow, Context, Error, Result};
use futures::lock::Mutex;
use futures::prelude::*;
use rand::distributions::Alphanumeric;
use rand::Rng;
use sha3::{Digest, Sha3_256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::iter::repeat_with;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{fs, io};
use tokio::task;
use url::{quirks::hostname, Position, Url};

use crate::rpc::BenchmarkOpt;
use ya_core_model::gftp as model;
use ya_core_model::identity;
use ya_core_model::net::{RemoteEndpoint, TryRemoteEndpoint};
use ya_core_model::NodeId;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub const DEFAULT_CHUNK_SIZE: u64 = 40 * 1024;

// =========================================== //
// File download - publisher side ("requestor")
// =========================================== //

struct BenchmarkDescr {
    hash: String,
    meta: model::GftpMetadata,
    rng: fastrand::Rng,
}

impl BenchmarkDescr {
    fn new(hash: String, meta: model::GftpMetadata) -> Arc<Self> {
        Arc::new(BenchmarkDescr {
            hash,
            meta,
            rng: fastrand::Rng::new(),
        })
    }

    pub fn open(name: &str) -> Result<Arc<BenchmarkDescr>> {
        let meta = model::GftpMetadata {
            file_size: 2000000000000,
        };
        let hash = {
            let mut hasher = Sha3_256::new();
            let _ = hasher.write_all(name.as_bytes());

            format!("{:x}", hasher.result())
        };

        Ok(BenchmarkDescr::new(hash, meta))
    }

    pub fn bind_handlers(self: &Arc<Self>) {
        let gsb_address = model::file_bus_id(&self.hash);
        let desc = self.clone();
        let _ = bus::bind(&gsb_address, move |_msg: model::GetMetadata| {
            log::debug!(
                "Received GetMetadata request. Returning metadata. {:?}",
                desc.meta
            );
            future::ok(desc.meta.clone())
        });

        let desc = self.clone();
        let _ = bus::bind(&gsb_address, move |msg: model::GetChunk| {
            let desc = desc.clone();
            async move { desc.get_chunk(msg.offset, msg.size).await }
        });
    }

    async fn get_chunk(
        &self,
        offset: u64,
        chunk_size: u64,
    ) -> Result<model::GftpChunk, model::Error> {
        let bytes_to_read = if self.meta.file_size - offset < chunk_size {
            self.meta.file_size - offset
        } else {
            chunk_size
        } as usize;

        log::debug!("Reading chunk at offset: {}, size: {}", offset, chunk_size);
        //let mut buffer = vec![0u8; bytes_to_read];
        let bytes: Vec<u8> = repeat_with(|| self.rng.u8(..))
            .take(bytes_to_read)
            .collect();

        Ok(model::GftpChunk {
            offset,
            content: bytes,
        })
    }
}

struct FileDesc {
    hash: String,
    file: Mutex<fs::File>,
    meta: model::GftpMetadata,
}

impl FileDesc {
    fn new(file: fs::File, hash: String, meta: model::GftpMetadata) -> Arc<Self> {
        let file = Mutex::new(file);

        Arc::new(FileDesc { hash, file, meta })
    }

    pub fn open(path: &Path) -> Result<Arc<FileDesc>> {
        let mut file =
            fs::File::open(path).with_context(|| format!("Can't open file {}.", path.display()))?;

        let hash = hash_file_sha256(&mut file)?;
        let meta = model::GftpMetadata {
            file_size: file.metadata()?.len(),
        };

        Ok(FileDesc::new(file, hash, meta))
    }

    pub fn bind_handlers(self: &Arc<Self>) {
        let gsb_address = model::file_bus_id(&self.hash);
        let desc = self.clone();
        let _ = bus::bind(&gsb_address, move |_msg: model::GetMetadata| {
            log::debug!(
                "Received GetMetadata request. Returning metadata. {:?}",
                desc.meta
            );
            future::ok(desc.meta.clone())
        });

        let desc = self.clone();
        let _ = bus::bind(&gsb_address, move |msg: model::GetChunk| {
            let desc = desc.clone();
            async move { desc.get_chunk(msg.offset, msg.size).await }
        });
    }

    async fn get_chunk(
        &self,
        offset: u64,
        chunk_size: u64,
    ) -> Result<model::GftpChunk, model::Error> {
        let bytes_to_read = if self.meta.file_size - offset < chunk_size {
            self.meta.file_size - offset
        } else {
            chunk_size
        } as usize;

        log::debug!("Reading chunk at offset: {}, size: {}", offset, chunk_size);
        let mut buffer = vec![0u8; bytes_to_read];
        {
            let mut file = self.file.lock().await;

            file.seek(SeekFrom::Start(offset)).map_err(|error| {
                model::Error::ReadError(format!("Can't seek file at offset {}, {}", offset, error))
            })?;

            file.read_exact(&mut buffer).map_err(|error| {
                model::Error::ReadError(format!(
                    "Can't read {} bytes at offset {}, error: {}",
                    bytes_to_read, offset, error
                ))
            })?;
        }

        Ok(model::GftpChunk {
            offset,
            content: buffer,
        })
    }
}

pub async fn publish(path: &Path) -> Result<Url> {
    let filedesc = FileDesc::open(path)?;
    filedesc.bind_handlers();

    gftp_url(&filedesc.hash).await
}

pub async fn publish_benchmark(str: &str) -> Result<Url> {
    let benchmark_descriptor = BenchmarkDescr::open(str)?;
    benchmark_descriptor.bind_handlers();

    gftp_url(&benchmark_descriptor.hash).await
}

pub async fn close(url: &Url) -> Result<bool> {
    let hash_name = match url.path_segments() {
        Some(segments) => match segments.last() {
            Some(segment) => segment,
            _ => return Err(anyhow!("Invalid URL: {:?}", url)),
        },
        _ => return Err(anyhow!("Invalid URL: {:?}", url)),
    };

    bus::unbind(model::file_bus_id(hash_name).as_str())
        .await
        .map_err(|e| anyhow!(e))
}

// =========================================== //
// File download - client side ("provider")
// =========================================== //

pub async fn download_from_url(url: &Url, dst_path: &Path) -> Result<()> {
    let (node_id, hash) = extract_url(url)?;
    download_file(node_id, &hash, dst_path).await
}

pub async fn download_file(node_id: NodeId, hash: &str, dst_path: &Path) -> Result<()> {
    let remote = node_id.service_transfer(&model::file_bus_id(hash));
    log::debug!("Creating target file {}", dst_path.display());

    let mut file = create_dest_file(dst_path)?;

    log::debug!("Loading file {} metadata.", dst_path.display());
    let metadata = remote.send(model::GetMetadata {}).await??;

    log::debug!("Metadata: file size {}.", metadata.file_size);

    let chunk_size = DEFAULT_CHUNK_SIZE;
    let num_chunks = (metadata.file_size + (chunk_size - 1)) / chunk_size; // Divide and round up.

    file.set_len(metadata.file_size)?;

    futures::stream::iter(0..num_chunks)
        .map(|chunk_number| {
            remote.call(model::GetChunk {
                offset: chunk_number * chunk_size,
                size: chunk_size,
            })
        })
        .buffered(12)
        .map_err(anyhow::Error::from)
        .try_for_each(move |result| {
            future::ready((|| {
                let chunk = result?;
                file.write_all(&chunk.content[..])?;
                Ok(())
            })())
        })
        .await?;

    Ok(())
}

// =========================================== //
// Benchmark download - client side
// =========================================== //

pub async fn download_benchmark_from_url(url: &Url, opt: &BenchmarkOpt) -> Result<()> {
    let (node_id, hash) = extract_url(url)?;
    download_benchmark(node_id, &hash, opt).await
}

pub async fn download_benchmark(node_id: NodeId, hash: &str, opt: &BenchmarkOpt) -> Result<()> {
    let total_time_start = Instant::now();

    let remote = node_id.service_transfer(&model::file_bus_id(hash));
    //   log::debug!("Creating target file {}", dst_path.display());

    // let mut file = create_dest_file(dst_path)?;

    //    log::debug!("Loading benchmark metadata");
    //  let metadata = remote.send(model::GetMetadata {}).await??;

    let max_bytes = opt.max_bytes;
    if let Some(max_bytes) = max_bytes {
        log::info!(
            "Benchmark will limit download size to {}",
            humansize::format_size(max_bytes, humansize::DECIMAL)
        );
    } else {
        log::info!("There will be no size limit on download test");
    }
    let chunk_size = opt.chunk_size;
    if chunk_size < 1 {
        return Err(anyhow!("Chunk size must be at least 1"));
    }
    let max_time_sec = opt.max_time_sec;
    if let Some(max_time_sec) = max_time_sec {
        log::info!("Benchmark will limit run time {} seconds", max_time_sec)
    } else {
        log::info!("There will be no time limit on download test");
    }

    let chunk_at_once = opt.chunk_at_once as usize;
    if chunk_at_once < 1 {
        return Err(anyhow!("Chunk at once must be at least 1"));
    }
    let num_chunks = if let Some(max_bytes) = max_bytes {
        (max_bytes + (chunk_size - 1)) / chunk_size // Divide and round up.
    } else {
        u64::MAX
    };

    let sum_bytes_ = Arc::new(AtomicU64::new(0));
    let sum_chunks_ = Arc::new(AtomicU64::new(0));
    let sum_bytes = sum_bytes_.clone();
    let sum_chunks = sum_chunks_.clone();

    let refresh_every_sec = opt.refresh_every_sec;
    let ts = tokio::spawn(async move {
        let mut last_sum_bytes = 0;
        let mut last_chunks = 0;
        let mut last_time = Instant::now();
        let refresh_every_seconds = refresh_every_sec;
        log::info!("Starting download progress loop. Refresh every {refresh_every_seconds}s");
        tokio::time::sleep(Duration::from_secs(1)).await;
        loop {
            let sum_bytes = sum_bytes.load(Ordering::SeqCst);
            let sum_chunks = sum_chunks.load(Ordering::SeqCst);
            let current_time = Instant::now();
            let elapsed = current_time - last_time;
            let bytes_per_sec = (sum_bytes - last_sum_bytes) as f64 / elapsed.as_secs_f64();
            let chunks_per_sec = (sum_chunks - last_chunks) as f64 / elapsed.as_secs_f64();

            last_sum_bytes = sum_bytes;
            last_chunks = sum_chunks;
            last_time = current_time;
            tokio::time::sleep(Duration::from_secs_f64(refresh_every_seconds)).await;
            log::info!(
                "Downloaded {} bytes in {} chunks, {}/s, {:.2} chunks/s",
                humansize::format_size(sum_bytes, humansize::DECIMAL),
                sum_chunks,
                humansize::format_size(bytes_per_sec as u64, humansize::DECIMAL),
                chunks_per_sec
            );
            if let Some(max_time_sec) = max_time_sec {
                if total_time_start.elapsed() > Duration::from_secs(max_time_sec as u64) {
                    log::info!("Max time reached, stopping");
                    break;
                }
            }
        }
    });
    let sum_bytes = sum_bytes_.clone();
    let sum_chunks = sum_chunks_.clone();

    let ts2 = task::spawn_local(async move {
        futures::stream::iter(0..num_chunks)
            .map(|chunk_number| {
                let chunk_size = if let Some(max_bytes) = max_bytes {
                    if chunk_number == num_chunks - 1 {
                        max_bytes - chunk_number * chunk_size
                    } else {
                        chunk_size
                    }
                } else {
                    chunk_size
                };
                remote.call(model::GetChunk {
                    offset: chunk_number * chunk_size,
                    size: chunk_size,
                })
            })
            .buffered(chunk_at_once)
            .map_err(anyhow::Error::from)
            .try_for_each(move |result| {
                let sum_bytes = sum_bytes.clone();
                let sum_chunks = sum_chunks.clone();
                future::ready((|| {
                    let chunk = result?;
                    let buf = &chunk.content[..];
                    sum_bytes.fetch_add(buf.len() as u64, Ordering::SeqCst);
                    sum_chunks.fetch_add(1, Ordering::SeqCst);
                    //log::debug!("Downloaded chunk: {:?}", sum_bytes.load(Ordering::SeqCst));
                    Ok(())
                })())
            })
            .await
    });

    tokio::select! {
        _ = ts => {
            log::info!("Download progress loop finished.");
        }
        _ = ts2 => {
            log::info!("Download task finished.");
        }
    }

    let sum_bytes = sum_bytes_.load(Ordering::SeqCst);
    let sum_chunks = sum_chunks_.load(Ordering::SeqCst);
    if let Some(max_bytes) = max_bytes {
        if sum_bytes != max_bytes {
            log::warn!("Downloaded {} bytes, expected {}", sum_bytes, max_bytes);
        }
    }
    println!(
        "Downloaded {} ({}B) in {} chunks, total time: {:.2}s",
        humansize::format_size(sum_bytes, humansize::DECIMAL),
        sum_bytes,
        sum_chunks,
        total_time_start.elapsed().as_secs_f64()
    );
    Ok(())
}

// =========================================== //
// File upload - publisher side ("requestor")
// =========================================== //

pub async fn open_for_upload(filepath: &Path) -> Result<Url> {
    let hash_name = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .map(char::from)
        .take(65)
        .collect::<String>();

    let file = Arc::new(Mutex::new(create_dest_file(filepath)?));

    let gsb_address = model::file_bus_id(&hash_name);
    let file_clone = file.clone();
    let _ = bus::bind(&gsb_address, move |msg: model::UploadChunk| {
        let file = file_clone.clone();
        async move { chunk_uploaded(file.clone(), msg).await }
    });

    let file_clone = file.clone();
    let _ = bus::bind(&gsb_address, move |msg: model::UploadFinished| {
        let file = file_clone.clone();
        async move { upload_finished(file.clone(), msg).await }
    });

    gftp_url(&hash_name).await
}

async fn chunk_uploaded(
    file: Arc<Mutex<File>>,
    msg: model::UploadChunk,
) -> Result<(), model::Error> {
    let mut file = file.lock().await;
    let chunk = msg.chunk;

    file.seek(SeekFrom::Start(chunk.offset)).map_err(|error| {
        model::Error::ReadError(format!(
            "Can't seek file at offset {}, {}",
            chunk.offset, error
        ))
    })?;
    file.write_all(&chunk.content[..]).map_err(|error| {
        model::Error::WriteError(format!(
            "Can't write {} bytes at offset {}, error: {}",
            chunk.content.len(),
            chunk.offset,
            error
        ))
    })?;
    Ok(())
}

async fn upload_finished(
    file: Arc<Mutex<File>>,
    msg: model::UploadFinished,
) -> Result<(), model::Error> {
    let mut file = file.lock().await;
    file.flush()
        .map_err(|error| model::Error::WriteError(format!("Can't flush file: {}", error)))?;

    if let Some(expected_hash) = msg.hash {
        log::debug!("Upload finished. Verifying hash...");

        let real_hash = hash_file_sha256(&mut file)
            .map_err(|error| model::Error::InternalError(error.to_string()))?;

        if expected_hash != real_hash {
            log::debug!(
                "Uploaded file hash {} is different than expected hash {}.",
                &real_hash,
                &expected_hash
            );
            //TODO: We should notify publisher about not matching hash.
            //      Now we send error only for uploader.
            return Err(model::Error::IntegrityError);
        }
        log::debug!("File hash matches expected hash {}.", &expected_hash);
    } else {
        log::debug!("Upload finished. Expected file hash not provided. Omitting validation.");
    }

    //TODO: unsubscribe gsb events.
    Ok(())
}

// =========================================== //
// File upload - client side ("provider")
// =========================================== //

pub async fn upload_file(path: &Path, url: &Url) -> Result<()> {
    let (node_id, random_filename) = extract_url(url)?;
    let remote = node_id.try_service(&model::file_bus_id(&random_filename))?;

    log::debug!("Opening file to send {}.", path.display());

    let chunk_size = DEFAULT_CHUNK_SIZE;

    futures::stream::iter(get_chunks(path, chunk_size)?)
        .map(|chunk| {
            let remote = remote.clone();
            async move {
                let chunk = chunk?;
                Ok::<_, anyhow::Error>(remote.call(model::UploadChunk { chunk }).await??)
            }
        })
        .buffered(3)
        .try_for_each(|_| future::ok(()))
        .await?;

    log::debug!("Computing file hash.");
    let hash = hash_file_sha256(&mut File::open(path)?)?;

    log::debug!("File [{}] has hash [{}].", path.display(), &hash);
    remote
        .call(model::UploadFinished { hash: Some(hash) })
        .await??;
    log::debug!("Upload finished correctly.");
    Ok(())
}

// =========================================== //
// Utils and common functions
// =========================================== //

fn get_chunks(
    file_path: &Path,
    chunk_size: u64,
) -> Result<impl Iterator<Item = Result<model::GftpChunk, std::io::Error>> + 'static, std::io::Error>
{
    let mut file = OpenOptions::new().read(true).open(file_path)?;

    let file_size = file.metadata()?.len();
    let n_chunks = (file_size + chunk_size - 1) / chunk_size;

    Ok((0..n_chunks).map(move |n| {
        let offset = n * chunk_size;
        let bytes_to_read = if offset + chunk_size > file_size {
            file_size - offset
        } else {
            chunk_size
        };
        let mut buffer = vec![0u8; bytes_to_read as usize];
        file.read_exact(&mut buffer)?;
        Ok(model::GftpChunk {
            offset,
            content: buffer,
        })
    }))
}

fn hash_file_sha256(mut file: &mut fs::File) -> Result<String> {
    let mut hasher = Sha3_256::new();

    file.seek(SeekFrom::Start(0))
        .with_context(|| "Can't seek file at offset 0.".to_string())?;
    io::copy(&mut file, &mut hasher)?;

    Ok(format!("{:x}", hasher.result()))
}

/// Returns NodeId and file hash from gftp url.
/// Note: In case of upload, hash is not real hash of file
/// but only cryptographically strong random string.
pub fn extract_url(url: &Url) -> Result<(NodeId, String)> {
    if url.scheme() != "gftp" {
        return Err(Error::msg(format!(
            "Unsupported url scheme {}.",
            url.scheme()
        )));
    }

    let node_id = NodeId::from_str(hostname(url))
        .with_context(|| format!("Url {} has invalid node_id.", url))?;

    // Note: Remove slash from beginning of path.
    let hash = &url[Position::BeforePath..Position::BeforeQuery][1..];
    Ok((node_id, hash.to_owned()))
}

async fn gftp_url(hash: &str) -> Result<Url> {
    let id = bus::service(identity::BUS_ID)
        .call(identity::Get::ByDefault)
        .await??
        .unwrap();

    Ok(Url::parse(&format!("gftp://{:?}/{}", id.node_id, hash))?)
}

fn ensure_dir_exists(file_path: &Path) -> Result<()> {
    if let Some(file_dir) = file_path.parent() {
        fs::create_dir_all(file_dir)?
    }
    Ok(())
}

fn create_dest_file(file_path: &Path) -> Result<File> {
    ensure_dir_exists(file_path).with_context(|| {
        format!(
            "Can't create destination directory for file: [{}].",
            file_path.display()
        )
    })?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(file_path)
        .with_context(|| format!("Can't create destination file: [{}].", file_path.display()))
}
