use fs2::FileExt;
use sha3::{Digest, Sha3_224, Sha3_256};
use std::convert::TryFrom;
use std::fs::{File, OpenOptions};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::path::{Component, PathBuf};

use crate::location::TransferHash;
use crate::{error::Error as TransferError, TransferUrl};

const VERIFIED_NAMESPACE: &str = "verified-v1";
const PARTIAL_NAMESPACE: &str = "partial-v1";

#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
    partial_dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        let verified_dir = dir.join(VERIFIED_NAMESPACE);
        let partial_dir = dir.join(PARTIAL_NAMESPACE);
        for path in [&verified_dir, &partial_dir] {
            std::fs::create_dir_all(path)
                .unwrap_or_else(|_| panic!("Unable to create directory: {}", path.display()));
        }
        Cache {
            dir: verified_dir,
            partial_dir,
        }
    }

    pub fn name(transfer_url: &TransferUrl) -> Result<CachePath, TransferError> {
        let hash = match &transfer_url.hash {
            Some(hash) => hash,
            None => return Err(TransferError::InvalidUrlError("hash required".to_owned())),
        };

        transfer_url.file_name()?;
        Ok(CachePath::new(hash.clone(), transfer_url.url.clone()))
    }

    #[inline(always)]
    pub fn to_partial_path(&self, path: &CachePath) -> ProjectedPath {
        ProjectedPath::local(self.partial_dir.clone(), path.partial_path())
    }

    pub async fn lock(&self, path: &CachePath) -> Result<CacheLock, TransferError> {
        let lock_path = self.partial_dir.join(path.lock_path());
        tokio::task::spawn_blocking(move || {
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(lock_path)?;
            file.lock_exclusive()?;
            Ok(CacheLock { _file: file })
        })
        .await
        .map_err(|error| TransferError::Other(format!("cache lock task failed: {error}")))?
    }

    #[inline(always)]
    pub fn to_final_path(&self, path: &CachePath) -> ProjectedPath {
        ProjectedPath::local(self.dir.clone(), path.final_path())
    }
}

/// Holds an advisory OS file lock for one canonical source URL and expected hash.
/// The lock is released automatically when the guard (or its process) exits.
pub struct CacheLock {
    _file: File,
}

impl TryFrom<ProjectedPath> for TransferUrl {
    type Error = TransferError;

    fn try_from(value: ProjectedPath) -> Result<Self, TransferError> {
        TransferUrl::parse(
            value
                .to_path_buf()
                .to_str()
                .ok_or_else(|| TransferError::InvalidUrlError("Invalid path".to_owned()))?,
            "file",
        )
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
    hash: TransferHash,
    partial_key: String,
}

impl CachePath {
    pub fn new(hash: TransferHash, source_url: url::Url) -> Self {
        let partial_key = partial_key(&source_url, &hash);
        CachePath { hash, partial_key }
    }

    pub fn partial_path(&self) -> PathBuf {
        format!("{}.partial", self.partial_key).into()
    }

    pub fn lock_path(&self) -> PathBuf {
        format!("{}.lock", self.partial_key).into()
    }

    /// Creates a source-independent final cache key for verified content.
    pub fn final_path(&self) -> PathBuf {
        content_key(&self.hash).into()
    }
}

fn partial_key(source_url: &url::Url, hash: &TransferHash) -> String {
    let mut canonical_url = source_url.clone();
    canonical_url.set_fragment(None);

    let mut digest = Sha3_256::default();
    digest_field(&mut digest, canonical_url.as_str().as_bytes());
    digest_field(&mut digest, hash.alg.as_bytes());
    digest_field(&mut digest, &hash.val);
    hex::encode(digest.result())
}

fn content_key(hash: &TransferHash) -> String {
    let mut digest = Sha3_224::default();
    digest_field(&mut digest, hash.alg.as_bytes());
    digest_field(&mut digest, &hash.val);
    hex::encode(digest.result())
}

fn digest_field<D: Digest>(digest: &mut D, bytes: &[u8]) {
    digest.input((bytes.len() as u64).to_be_bytes());
    digest.input(bytes);
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

    fn transfer_url(source: &str, hash: &str) -> TransferUrl {
        TransferUrl::parse_with_hash(&format!("hash:sha3:{hash}:{source}"), "file").unwrap()
    }

    #[test]
    fn partial_key_is_deterministic_and_binds_url_algorithm_and_hash() {
        let hash_a = "11".repeat(32);
        let hash_b = "22".repeat(32);
        let first = Cache::name(&transfer_url("https://example.com/image.gvmi", &hash_a)).unwrap();
        let same = Cache::name(&transfer_url("https://example.com/image.gvmi", &hash_a)).unwrap();
        let other_url =
            Cache::name(&transfer_url("https://mirror.example/image.gvmi", &hash_a)).unwrap();
        let other_file_name =
            Cache::name(&transfer_url("https://mirror.example/renamed.bin", &hash_a)).unwrap();
        let other_hash =
            Cache::name(&transfer_url("https://example.com/image.gvmi", &hash_b)).unwrap();
        let mut other_algorithm_url = transfer_url("https://example.com/image.gvmi", &hash_a);
        other_algorithm_url.hash.as_mut().unwrap().alg = "sha2".to_string();
        let other_algorithm = Cache::name(&other_algorithm_url).unwrap();
        let with_fragment = Cache::name(&transfer_url(
            "https://example.com/image.gvmi#not-sent-to-server",
            &hash_a,
        ))
        .unwrap();

        assert_eq!(first.partial_path(), same.partial_path());
        assert_eq!(first.partial_path(), with_fragment.partial_path());
        assert_ne!(first.partial_path(), other_url.partial_path());
        assert_ne!(first.partial_path(), other_file_name.partial_path());
        assert_ne!(first.partial_path(), other_hash.partial_path());
        assert_ne!(first.partial_path(), other_algorithm.partial_path());
        assert_eq!(first.final_path(), other_url.final_path());
        assert_eq!(first.final_path(), other_file_name.final_path());
        assert!(!first
            .partial_path()
            .to_string_lossy()
            .contains("example.com"));
    }

    #[test]
    fn legacy_entries_are_outside_the_trusted_namespace() {
        let root = tempdir::TempDir::new("transfer-cache-namespace").unwrap();
        let cache = Cache::new(root.path().to_path_buf());
        let raw_hash = "11".repeat(32);
        let cache_path =
            Cache::name(&transfer_url("https://example.com/image.gvmi", &raw_hash)).unwrap();
        let legacy_path = root.path().join(format!("image_{raw_hash}.gvmi"));
        std::fs::write(&legacy_path, b"legacy unverified entry").unwrap();

        let trusted_path = cache.to_final_path(&cache_path).to_path_buf();
        assert!(legacy_path.exists());
        assert!(!trusted_path.exists());
        assert!(trusted_path.starts_with(root.path().join(VERIFIED_NAMESPACE)));
    }

    #[actix_rt::test]
    async fn same_key_lock_serializes_callers_without_truncating_partial() {
        let root = tempdir::TempDir::new("transfer-cache-lock").unwrap();
        let cache = Cache::new(root.path().to_path_buf());
        let cache_path = Cache::name(&transfer_url(
            "https://example.com/image.gvmi",
            &"11".repeat(32),
        ))
        .unwrap();
        let partial_path = cache.to_partial_path(&cache_path).to_path_buf();
        std::fs::write(&partial_path, b"partial contents").unwrap();

        let first = cache.lock(&cache_path).await.unwrap();
        let second_cache = cache.clone();
        let second_path = cache_path.clone();
        let mut second = Box::pin(second_cache.lock(&second_path));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut second)
                .await
                .is_err()
        );

        drop(first);
        let _second = tokio::time::timeout(std::time::Duration::from_secs(2), second)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(partial_path).unwrap(), b"partial contents");
    }
}
