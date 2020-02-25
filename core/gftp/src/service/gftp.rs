use futures::lock::Mutex;
use std::sync::Arc;


struct FileDesc {
    path: PathBuf,
    hash: String,
}


pub struct GftpService {
    files: Vec<FileDesc>,
}

impl GftpService {
    pub fn new() -> Arc<Mutex<GftpService>> {
        Arc::new(Mutex::new(GftpService{files: vec![]}))
    }

    pub fn publish_file(path: &Path) -> Result<String> {

    }
}

