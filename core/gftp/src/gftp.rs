use anyhow::{Context, Result};
use futures::lock::Mutex;
use futures::prelude::*;
use log::{debug, info};
use sha3::{Digest, Sha3_256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::{fs, io};
use ya_core_model::gftp as model;
use ya_service_bus::{typed as bus, RpcEndpoint};

struct FileDesc {
    hash: String,
    file: Mutex<fs::File>,
    meta: model::GftpMetadata,
}

#[derive(Clone)]
pub struct Config {
    pub chunk_size: u64,
}

impl Config {
    pub async fn publish(&self, path: &Path) -> Result<String> {
        let filedesc = FileDesc::open(path, self)?;
        filedesc.bind_handlers();
        Ok(filedesc.hash.clone())
    }
}

impl FileDesc {
    fn new(file: fs::File, hash: String, meta: model::GftpMetadata) -> Arc<Self> {
        let file = Mutex::new(file);

        Arc::new(FileDesc { hash, file, meta })
    }

    pub fn open(path: &Path, config: &Config) -> Result<Arc<FileDesc>> {
        let mut file = fs::File::open(&path)
            .with_context(|| format!("Can't open file {}.", path.display()))?;

        let hash = hash_file_sha256(&mut file)?;
        let meta = meta_from_file(&file, &config)?;

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
            async move { desc.get_chunk(msg.chunk_number).await }
        });
    }

    async fn get_chunk(&self, chunk_num: u64) -> Result<model::GftpChunk, model::Error> {
        let chunk_size = self.meta.chunk_size;
        let offset = chunk_size * chunk_num;

        let bytes_to_read = if self.meta.file_size - offset < chunk_size {
            self.meta.file_size - offset
        } else {
            chunk_size
        } as usize;

        debug!("Reading chunk at offset {}", offset);
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

        Ok(model::GftpChunk { content: buffer })
    }
}

fn meta_from_file(file: &fs::File, config: &Config) -> Result<model::GftpMetadata> {
    let metadata = file.metadata()?;

    let file_size = metadata.len();
    let num_chunks = (file_size + (config.chunk_size - 1)) / config.chunk_size; // Divide and round up.

    Ok(model::GftpMetadata {
        chunk_size: config.chunk_size,
        file_size,
        chunks_num: num_chunks,
    })
}

fn hash_file_sha256(mut file: &mut fs::File) -> Result<String> {
    let mut hasher = Sha3_256::new();
    //hasher.input(file);
    io::copy(&mut file, &mut hasher)?;

    Ok(format!("{:x}", hasher.result()))
}

pub async fn download_file(gsb_path: &str, dst_path: &Path) -> Result<()> {
    let remote = bus::service(gsb_path);
    debug!("Creating target file {}", dst_path.display());

    let mut file = create_dest_file(dst_path)?;

    info!("Loading file {} metadata.", dst_path.display());
    let metadata = remote.send(model::GetMetadata {}).await??;

    // GftpService::load_metadata(gsb_path).await?;
    debug!(
        "Metadata: file size {}, number of chunks {}, chunk size {}.",
        metadata.file_size, metadata.chunks_num, metadata.chunk_size
    );

    let num_chunks = metadata.chunks_num;
    //file.set_len(metadata.file_size)?;

    futures::stream::iter(0..num_chunks)
        .map(|chunk_number| remote.call(model::GetChunk { chunk_number }))
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
