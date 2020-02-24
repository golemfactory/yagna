use std::path::{PathBuf, Path};
use url::Url;



pub struct ContentHash {
    pub algorithm: String,
    pub digest: String,
}


pub struct Cache {
    cachedir: PathBuf,
}


impl Cache {
    pub fn new(cachedir: &Path) -> Cache {
        Cache{cachedir: cachedir.to_path_buf()}
    }

    pub fn get_dir(&self) -> &Path {
        &self.cachedir
    }

    pub fn find_in_cache(&self, url: &Url, hash: &ContentHash) -> Option<PathBuf> {
        unimplemented!();
    }
}

