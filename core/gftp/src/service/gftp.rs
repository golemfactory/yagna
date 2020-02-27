use anyhow::{Result, Error, Context};
use log::{info, debug};
use futures::lock::Mutex;
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;
use std::{fs, io};
use std::path::{PathBuf, Path};
use std::sync::Arc;
use std::fs::File;
use std::io::{Read, Write, SeekFrom, Seek};


use ya_service_bus::{typed as bus, RpcEndpoint};
use ya_core_model::gftp as model;
use ya_core_model::gftp::GftpMetadata;


struct FileDesc {
    path: PathBuf,
    hash: String,
    file: fs::File,
    meta: model::GftpMetadata,
}

#[derive(Clone)]
pub struct GftpConfig {
    pub chunk_size: u64,
}

pub struct GftpService {
    files: HashMap<String, Arc<Mutex<FileDesc>>>,
    config: GftpConfig,
}

impl FileDesc {
    fn new(path: &Path, file: fs::File, hash: String, meta: model::GftpMetadata) -> Arc<Mutex<FileDesc>> {
        Arc::new(Mutex::new(FileDesc{path: path.to_owned(), hash: hash.to_string(), file, meta}))
    }

    pub fn open(path: &Path, config: &GftpConfig) -> Result<Arc<Mutex<FileDesc>>> {
        let mut file = fs::File::open(&path)
            .with_context(|| format!("Can't open file {}.", path.display()))?;

        let hash = Self::hash_file_sha256(&mut file)?;
        let meta = Self::meta_from_file(&file, &config)?;

        Ok(FileDesc::new(path, file, hash, meta))
    }

    pub fn meta_from_file(file: &fs::File, config: &GftpConfig) -> Result<model::GftpMetadata> {
        let metadata = file.metadata()?;

        let file_size = metadata.len();
        let num_chunks = (file_size + (config.chunk_size - 1)) / config.chunk_size;     // Divide and round up.

        Ok(model::GftpMetadata{chunk_size: config.chunk_size, file_size, chunks_num: num_chunks})
    }

    fn hash_file_sha256(mut file: &mut fs::File) -> Result<String> {
        let mut hasher = Sha3_256::new();
        //hasher.input(file);
        io::copy(&mut file, &mut hasher)?;

        Ok(format!("{:x}", hasher.result()))
    }
}



impl GftpService {
    pub fn new(config: GftpConfig) -> Arc<Mutex<GftpService>> {
        Arc::new(Mutex::new(GftpService{files: HashMap::new(), config}))
    }

    pub async fn publish_file(me: Arc<Mutex<GftpService>>, path: &Path) -> Result<String> {
        let config = me.lock().await.config.clone();

        let filedesc = FileDesc::open(path, &config)?;
        let hash = filedesc.lock().await.hash.clone();

        let gsb_address = format!("{}/{}", model::BUS_ID, &hash);
        Self::bind_gsb_handlers(&gsb_address, filedesc.clone());

        Ok(hash)
    }

    pub async fn download_file(_me: Arc<Mutex<GftpService>>, gsb_path: &str, dst_path: &Path) -> Result<()> {
        debug!("Creating target file {}", dst_path.display());

        let mut file = GftpService::create_dest_file(dst_path)?;

        info!("Loading file {} metadata.", dst_path.display());
        let metadata = GftpService::load_metadata(gsb_path).await?;
        debug!("Metadata: file size {}, number of chunks {}, chunk size {}.", metadata.file_size, metadata.chunks_num, metadata.chunk_size);

        let num_chunks = metadata.chunks_num;
        file.set_len(metadata.file_size)?;

        for chunk_idx in 0..num_chunks {
            debug!("Loading chunk {} of file {}.", chunk_idx, dst_path.display());

            let chunk = GftpService::download_chunk(gsb_path, chunk_idx).await?;
            let written = file.write(&chunk.content[..])?;

            if written != chunk.content.len() {
                return Err(Error::msg(format!("Less bytes written to file, than got from gsb.")));
            }
        }

        Ok(())
    }

    async fn load_metadata(gsb_path: &str) -> Result<GftpMetadata> {
        Ok(bus::service(gsb_path)
            .send(model::GetMetadata{})
            .await
            .map_err(anyhow::Error::msg)?
            .map_err(anyhow::Error::msg)?)
    }

    async fn download_chunk(gsb_path: &str, chunk_idx: u64) -> Result<model::GftpChunk> {
        let msg = model::GetChunk{chunk_number: chunk_idx};

        Ok(bus::service(gsb_path)
            .send(msg)
            .await
            .map_err(anyhow::Error::msg)?
            .map_err(anyhow::Error::msg)?)
    }

    async fn get_metadata(desc: Arc<Mutex<FileDesc>>) -> Result<model::GftpMetadata, model::Error> {
        Ok(desc.lock().await.meta.clone())
    }

    async fn get_chunk(desc: Arc<Mutex<FileDesc>>, chunk_num: u64) -> Result<model::GftpChunk, model::Error> {
        let mut desc = desc.lock().await;
        let chunk_size = desc.meta.chunk_size;
        let offset = chunk_size * chunk_num;

        let bytes_to_read = if desc.meta.file_size - offset < chunk_size {
            desc.meta.file_size - offset
        } else {
            chunk_size
        } as usize;

        debug!("Reading chunk at offset {}", offset);

        desc.file.seek(SeekFrom::Start(offset))
            .map_err(|error| model::Error::ReadError(format!("Can't seek file at offset {}, {}", offset, error)))?;

        let mut buffer = vec![0u8; bytes_to_read];

        desc.file.read_exact(&mut buffer)
            .map_err(|error| model::Error::ReadError(format!("Can't read {} bytes at offset {}, error: {}", bytes_to_read, offset, error)))?;

        Ok(model::GftpChunk{content: buffer})
    }

    fn bind_gsb_handlers(gsb_address: &str, filedesc: Arc<Mutex<FileDesc>>) {
        let desc = filedesc.clone();
        let _ = bus::bind(&gsb_address, move |_msg: model::GetMetadata| {
            let filedesc = desc.clone();
            async move {
                GftpService::get_metadata(filedesc).await
            }
        });

        let desc = filedesc.clone();
        let _ = bus::bind(&gsb_address, move |msg: model::GetChunk| {
            let filedesc = desc.clone();
            async move {
                GftpService::get_chunk(filedesc, msg.chunk_number).await
            }
        });
    }

    fn ensure_dir_exists(file_path: &Path) -> Result<()> {
        let mut dir = file_path.to_path_buf();
        dir.pop();
        Ok(fs::create_dir_all(&dir)?)
    }

    fn create_dest_file(file_path: &Path) -> Result<File> {
        GftpService::ensure_dir_exists(file_path)
            .with_context(|| format!("Can't create destination directory for file: [{}].", file_path.display()))?;
        Ok(File::create(file_path)
            .with_context(|| format!("Can't create destination file: [{}].", file_path.display()))?)
    }
}

