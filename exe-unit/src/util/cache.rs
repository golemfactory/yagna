use crate::error::{Error, TransferError};
use sha3::Digest;
use std::convert::TryFrom;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::path::{Component, PathBuf};
use ya_transfer::TransferUrl;

#[derive(Debug, Clone)]
pub(crate) struct Cache {
    dir: PathBuf,
    #[allow(dead_code)]
    tmp_dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        let tmp_dir = dir.join("tmp");
        std::fs::create_dir_all(&tmp_dir)
            .unwrap_or_else(|_| panic!("Unable to create directory: {}", tmp_dir.display()));
        Cache { dir, tmp_dir }
    }

    pub fn name(transfer_url: &TransferUrl) -> Result<CachePath, TransferError> {
        let hash = match &transfer_url.hash {
            Some(hash) => hash,
            None => return Err(TransferError::InvalidUrlError("hash required".to_owned())),
        };

        let name = transfer_url.file_name()?;
        let location_hash = {
            let bytes = transfer_url.url.as_str().as_bytes();
            let hash = sha3::Sha3_224::digest(bytes);
            hex::encode(hash)
        };

        Ok(CachePath::new(name.into(), hash.val.clone(), location_hash))
    }

    #[inline(always)]
    #[cfg(not(feature = "sgx"))]
    pub fn to_temp_path(&self, path: &CachePath) -> ProjectedPath {
        ProjectedPath::local(self.tmp_dir.clone(), path.temp_path())
    }

    #[inline(always)]
    pub fn to_final_path(&self, path: &CachePath) -> ProjectedPath {
        ProjectedPath::local(self.dir.clone(), path.final_path())
    }
}

impl TryFrom<ProjectedPath> for TransferUrl {
    type Error = Error;

    fn try_from(value: ProjectedPath) -> Result<Self, Error> {
        TransferUrl::parse(
            value.to_path_buf().to_str().ok_or_else(|| {
                Error::local(TransferError::InvalidUrlError("Invalid path".to_owned()))
            })?,
            "file",
        )
        .map_err(Error::local)
    }
}

#[derive(Clone, Debug)]
pub enum ProjectedPath {
    Local { dir: PathBuf, path: PathBuf },
    Container { path: PathBuf },
}

impl ProjectedPath {
    pub fn local(dir: PathBuf, path: PathBuf) -> Self {
        ProjectedPath::Local {
            dir,
            path: flatten_container_path(path),
        }
    }

    pub fn container(path: PathBuf) -> Self {
        ProjectedPath::Container {
            path: flatten_container_path(path),
        }
    }
}

impl ProjectedPath {
    pub fn create_dir_all(&self) -> std::result::Result<(), IoError> {
        if let ProjectedPath::Container { .. } = &self {
            return Err(IoError::from(IoErrorKind::InvalidInput));
        }

        let path = self.to_path_buf();
        let parent = match path.parent() {
            Some(parent) => parent,
            None => return Ok(()),
        };

        if let Err(error) = std::fs::create_dir_all(parent) {
            match &error.kind() {
                std::io::ErrorKind::AlreadyExists => (),
                _ => return Err(error),
            }
        }

        Ok(())
    }

    pub fn to_path_buf(&self) -> PathBuf {
        match self {
            ProjectedPath::Local { dir, path } => {
                dir.clone().join(remove_container_path_base(path.clone()))
            }
            ProjectedPath::Container { path } => path.clone(),
        }
    }

    pub fn to_local(&self, dir: PathBuf) -> Self {
        match self {
            ProjectedPath::Local { dir: _, path } => ProjectedPath::Local {
                dir,
                path: path.clone(),
            },
            ProjectedPath::Container { path } => ProjectedPath::Local {
                dir,
                path: path.clone(),
            },
        }
    }
}

impl From<CachePath> for PathBuf {
    fn from(cache_path: CachePath) -> Self {
        cache_path.final_path()
    }
}

#[derive(Clone, Debug)]
pub struct CachePath {
    path: PathBuf,
    hash: Vec<u8>,
    nonce: String,
}

impl CachePath {
    pub fn new(path: PathBuf, hash: Vec<u8>, nonce: String) -> Self {
        CachePath { path, hash, nonce }
    }
    /// Creates the long version of path, including hash and the "random" token.
    pub fn temp_path(&self) -> PathBuf {
        let mut digest = sha3::Sha3_224::default();
        digest.input(&self.hash);
        digest.input(&self.nonce);
        let hash = digest.result();
        PathBuf::from(hex::encode(hash))
    }

    /// Creates a shorter version of path, including hash and excluding the "random" token.
    pub fn final_path(&self) -> PathBuf {
        let stem = self.path.file_stem().unwrap();
        let extension = self.path.extension();
        let hash = hex::encode(&self.hash);

        let mut file_name = stem.to_os_string();
        file_name.push("_");
        file_name.push(hash);

        if let Some(ext) = extension {
            file_name.push(".");
            file_name.push(ext);
        }

        file_name.into()
    }
}

/// Path flattening specific to the custom "container" scheme. Naively resolves all occurrences of
/// ".." and strips all ".". Does not support symlinks.
fn flatten_container_path(path: PathBuf) -> PathBuf {
    let mut result = Vec::new();
    for component in path.components() {
        match &component {
            Component::CurDir => continue,
            Component::ParentDir => {
                result.pop();
            }
            _ => {
                result.push(component);
            }
        }
    }
    result.into_iter().collect::<PathBuf>()
}

/// Remove the root dir and all prefixes from a path. Specific to the custom "container" scheme.
fn remove_container_path_base(path: PathBuf) -> PathBuf {
    path.components()
        .filter(|c| !matches!(c, Component::RootDir | Component::Prefix(_)))
        .collect::<PathBuf>()
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::path::PathBuf;
    use std::str::FromStr;

    fn path_buf<S: AsRef<str>>(s: S) -> PathBuf {
        PathBuf::from_str(s.as_ref()).unwrap()
    }

    #[test]
    fn test_flatten_path() {
        assert_eq!(
            path_buf("path"),
            flatten_container_path(path_buf("./some/../path"))
        );
        assert_eq!(
            path_buf("some/path"),
            flatten_container_path(path_buf("./some/./other/../path"))
        );
        assert_eq!(
            path_buf("/some/path"),
            flatten_container_path(path_buf("/some/./other/../path"))
        );
        assert_eq!(
            path_buf("/some/other/path"),
            flatten_container_path(path_buf("/some/other/path"))
        );
    }

    #[test]
    fn test_remove_base() {
        assert_eq!(path_buf(""), remove_container_path_base(path_buf("")));
        assert_eq!(path_buf(""), remove_container_path_base(path_buf("/")));
        assert_eq!(
            path_buf("some/directory"),
            remove_container_path_base(path_buf("/some/directory"))
        );
        assert_eq!(
            path_buf("another/directory"),
            remove_container_path_base(path_buf("another/directory"))
        );
    }
}
