use anyhow::{Result, Error, Context};
use sha3::{Digest, Sha3_256};
use futures::lock::Mutex;
use std::collections::HashMap;
use std::{fs, io};
use std::path::{PathBuf, Path};
use std::sync::Arc;


use ya_service_bus::{typed as bus};
use ya_core_model::gftp as model;


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

    fn bind_gsb_handlers(gsb_address: &str, filedesc: Arc<Mutex<FileDesc>>) {
        let desc = filedesc.clone();
        let _ = bus::bind(&gsb_address, move |msg: model::GetMetadata| {
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

    async fn get_metadata(desc: Arc<Mutex<FileDesc>>) -> Result<model::GftpMetadata, model::Error> {
        Ok(desc.lock().await.meta.clone())
    }

    async fn get_chunk(desc: Arc<Mutex<FileDesc>>, chunk_num: u64) -> Result<model::GftpChunk, model::Error> {
        //let desc = desc.lock().await;

        Err(model::Error::ReadError)
    }
}

