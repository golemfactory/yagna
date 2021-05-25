use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::path::{Component, PathBuf};

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
        self.to_path_buf(true, true)
    }

    /// Creates a shorter version of path, including hash and excluding the "random" token.
    pub fn final_path(&self) -> PathBuf {
        self.to_path_buf(true, false)
    }

    fn to_path_buf(&self, with_hash: bool, with_nonce: bool) -> PathBuf {
        let stem = self.path.file_stem().unwrap();
        let extension = self.path.extension();
        let hash = hex::encode(&self.hash);

        let mut file_name = stem.to_os_string();

        if with_hash {
            file_name.push("_");
            file_name.push(hash);
        }
        if with_nonce {
            file_name.push("_");
            file_name.push(&self.nonce);
        }
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
        .filter(|c| match c {
            Component::RootDir => false,
            Component::Prefix(_) => false,
            _ => true,
        })
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
