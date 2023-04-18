use anyhow::{anyhow, Context, Error, Result};
use futures::lock::Mutex;
use futures::prelude::*;
use rand::distributions::Alphanumeric;
use rand::Rng;
use sha3::{Digest, Sha3_256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::{fs, io};
use url::{quirks::hostname, Position, Url};

use ya_core_model::gftp as model;
use ya_core_model::identity;
use ya_core_model::net::{RemoteEndpoint, TryRemoteEndpoint};
use ya_core_model::NodeId;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub const DEFAULT_CHUNK_SIZE: u64 = 40 * 1024;

// =========================================== //
// File download - publisher side ("requestor")
// =========================================== //

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
