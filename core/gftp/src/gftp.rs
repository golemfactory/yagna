use anyhow::{Context, Error, Result};
use futures::lock::Mutex;
use futures::prelude::*;
use log::{debug, info};
use rand::Rng;
use rand::distributions::Alphanumeric;
use sha3::{Digest, Sha3_256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::{fs, io};
use url::{Position, Url, quirks::hostname};

use ya_core_model::gftp as model;
use ya_core_model::{ethaddr::NodeId, identity};
use ya_net::RemoteEndpoint;
use ya_service_bus::{typed as bus, RpcEndpoint};


const DEFAULT_CHUNK_SIZE: u64 = 40 * 1024;

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
        let mut file = fs::File::open(&path)
            .with_context(|| format!("Can't open file {}.", path.display()))?;

        let hash = hash_file_sha256(&mut file)?;
        let meta = model::GftpMetadata { file_size: file.metadata()?.len() };

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

    async fn get_chunk(&self, offset: u64, chunk_size: u64) -> Result<model::GftpChunk, model::Error> {
        let bytes_to_read = if self.meta.file_size - offset < chunk_size {
            self.meta.file_size - offset
        } else {
            chunk_size
        } as usize;

        debug!("Reading chunk at offset: {}, size: {}", offset, chunk_size);
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

        Ok(model::GftpChunk { offset, content: buffer })
    }
}

pub async fn publish(path: &Path) -> Result<Url> {
    let filedesc = FileDesc::open(path)?;
    filedesc.bind_handlers();

    Ok(gftp_url(&filedesc.hash).await?)
}

// =========================================== //
// File download - client side ("provider")
// =========================================== //

pub async fn download_from_url(url: &Url, dst_path: &Path) -> Result<()> {
    let (node_id, hash) = extract_url(url)?;
    download_file(node_id, &hash, dst_path).await
}

pub async fn download_file(node_id: NodeId, hash: &str, dst_path: &Path) -> Result<()> {
    let remote = node_id.service(&model::file_bus_id(hash));
    debug!("Creating target file {}", dst_path.display());

    let mut file = create_dest_file(dst_path)?;

    info!("Loading file {} metadata.", dst_path.display());
    let metadata = remote.send(model::GetMetadata {}).await??;

    debug!("Metadata: file size {}.", metadata.file_size);

    let chunk_size = DEFAULT_CHUNK_SIZE;
    let num_chunks = (metadata.file_size + (chunk_size - 1)) / chunk_size; // Divide and round up.

    file.set_len(metadata.file_size)?;

    futures::stream::iter(0..num_chunks)
        .map(|chunk_number| remote.call(model::GetChunk { offset: chunk_number * chunk_size, size: chunk_size }))
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

pub async fn open_for_upload(directory: &Path) -> Result<(PathBuf, Url)> {
    let filename = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(65)
        .collect::<String>();

    let filepath = directory.join(&filename);
    let file = Arc::new(Mutex::new(create_dest_file(&filepath)?));

    let gsb_address = model::file_bus_id(&filename);
    let file_clone = file.clone();
    let _ = bus::bind(&gsb_address, move |msg: model::UploadChunk| {
        let file = file_clone.clone();
        async move { Ok(chunk_uploaded(file.clone(), msg).await?) }
    });

    let file_clone = file.clone();
    let _ = bus::bind(&gsb_address, move |msg: model::UploadFinished| {
        let file = file_clone.clone();
        async move { Ok(upload_finished(file.clone(), msg).await?) }
    });

    Ok((filepath, gftp_url(&filename).await?))
}

async fn chunk_uploaded(file: Arc<Mutex<File>>, msg: model::UploadChunk) -> Result<(), model::Error> {
    let mut file = file.lock().await;
    let chunk = msg.chunk;

    file.seek(SeekFrom::Start(chunk.offset)).map_err(|error| {
        model::Error::ReadError(format!("Can't seek file at offset {}, {}", chunk.offset, error))
    })?;
    file.write_all(&chunk.content[..]).map_err(|error| {
        model::Error::ReadError(format!(
            "Can't read {} bytes at offset {}, error: {}",
            chunk.content.len(), chunk.offset, error
        ))
    })?;
    Ok(())
}

async fn upload_finished(_file: Arc<Mutex<File>>, _msg: model::UploadFinished) -> Result<(), model::Error> {
    //TODO: unsubscribe gsb events.
    //TODO: compare hash of disk file against expected hash from message.
    unimplemented!();
}

// =========================================== //
// File upload - client side ("provider")
// =========================================== //

pub async fn upload_file(path: &Path, url: &Url) -> Result<()> {
    let (node_id, random_filename) = extract_url(url)?;
    let remote = node_id.service(&model::file_bus_id(&random_filename));

    debug!("Opening file to send {}.", path.display());
    let file = File::open(path)
        .with_context(|| format!("Can't upload file: [{}].", path.display()))?;

    let filesize = file.metadata()?.len();
    let chunk_size = DEFAULT_CHUNK_SIZE;
    let num_chunks = (filesize + (chunk_size - 1)) / chunk_size; // Divide and round up.
    let file = Arc::new(Mutex::new(file));

//    futures::stream::iter(0..num_chunks)
//        .map(|chunk_number| {
//            let offset = chunk_number * chunk_size;
//            get_chunk(file, offset, chunk_size, filesize)
//        })
//        .try_for_each_concurrent(12, async move |chunk| {
//            let chunk = chunk?;
//            Ok(remote.call(model::UploadChunk { chunk }).await?)
//        }).await?;

    for chunk_number in 0..num_chunks {
        let offset = chunk_number * chunk_size;
        let chunk = get_chunk(file.clone(), offset, chunk_size, filesize).await?;

        remote.call(model::UploadChunk { chunk }).await??;
    }

    Ok(())
}

// =========================================== //
// Utils and common functions
// =========================================== //

async fn get_chunk(file: Arc<Mutex<File>>, offset: u64, chunk_size: u64, filesize: u64) -> Result<model::GftpChunk, model::Error> {
    let bytes_to_read = if filesize - offset < chunk_size {
        filesize - offset
    } else {
        chunk_size
    } as usize;

    debug!("Reading chunk at offset: {}, size: {}", offset, chunk_size);
    let mut buffer = vec![0u8; bytes_to_read];
    {
        let mut file = file.lock().await;

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

    Ok(model::GftpChunk { offset, content: buffer })
}

fn hash_file_sha256(mut file: &mut fs::File) -> Result<String> {
    let mut hasher = Sha3_256::new();
    io::copy(&mut file, &mut hasher)?;

    Ok(format!("{:x}", hasher.result()))
}

/// Returns NodeId and file hash from gftp url.
/// Note: In case of upload, hash is not real hash of file
/// but only cryptographically strong random string.
fn extract_url(url: &Url) -> Result<(NodeId, String)> {
    if url.scheme() != "gftp" {
        return Err(Error::msg(format!(
            "Unsupported url scheme {}.",
            url.scheme()
        )));
    }

    let node_id = NodeId::from_str(hostname(&url))
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

    Ok(Url::parse(&format!(
        "gftp://{:?}/{}",
        id.node_id, hash
    ))?)
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
    Ok(File::create(file_path)
        .with_context(|| format!("Can't create destination file: [{}].", file_path.display()))?)
}
