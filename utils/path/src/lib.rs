use std::ffi::OsString;
use std::io;
use std::io::Write;
use std::path::{Component, Path, PathBuf, Prefix};

pub mod data_dir;

/// Adds security layer to paths manipulation to avoid popular attacks.
pub trait SecurePath {
    /// Joins path component stripping it from all potentially insecure
    /// characters to avoid path traversal attacks.
    fn secure_join<PathRef: AsRef<Path>>(&self, path: PathRef) -> PathBuf;
}

impl<T> SecurePath for T
where
    T: AsRef<Path>,
{
    fn secure_join<PathRef: AsRef<Path>>(&self, path: PathRef) -> PathBuf {
        let append = remove_insecure_chars(path);
        self.as_ref().join(&append)
    }
}

pub trait SwapSave {
    fn swap_save<B: AsRef<[u8]>>(&self, bytes: B) -> io::Result<()>;
}

impl<T> SwapSave for T
where
    T: AsRef<Path>,
{
    fn swap_save<B: AsRef<[u8]>>(&self, bytes: B) -> io::Result<()> {
        let path = self.as_ref();
        let extension = match path.extension() {
            Some(ext) => {
                let mut ext = ext.to_os_string();
                ext.push(".swp");
                ext
            }
            _ => OsString::from("swp"),
        };
        let path_tmp = path.to_path_buf().with_extension(extension);

        let mut file = std::fs::File::create(&path_tmp)?;
        file.write_all(bytes.as_ref())?;
        file.flush()?;
        drop(file);

        std::fs::rename(path_tmp, path)?;

        Ok(())
    }
}

/// Canonicalize and strip the path of prefix components (Windows)
pub fn normalize_path<P: AsRef<Path>>(path: P) -> std::io::Result<PathBuf> {
    let path = path.as_ref().canonicalize()?;
    if !cfg!(windows) {
        return Ok(path);
    }

    // canonicalize on Windows adds `\\?` (or `%3f` when url-encoded) prefix
    let mut components = path.components();
    let path = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => path,
            Prefix::VerbatimDisk(disk) => {
                let mut p = OsString::from(format!("{}:", disk as char));
                p.push(components.as_path());
                PathBuf::from(p)
            }
            _ => panic!("Invalid path: {:?}", path),
        },
        _ => path,
    };

    Ok(path)
}

#[allow(clippy::unnecessary_filter_map)]
fn remove_insecure_chars<PathRef: AsRef<Path>>(path: PathRef) -> PathBuf {
    let path = path.as_ref().to_path_buf();
    path.components()
        .filter_map(|component| {
            match component {
                // Joining absolute path, overrides previous path, leaving
                // only appended part. Insecure.
                Component::RootDir | Component::Prefix { .. } => None,
                // Allows to travers up in directory hierarchy to root. Insecure.
                Component::ParentDir { .. } => None,
                // I don't what effect would be, so better not allow it.
                Component::CurDir => None,
                Component::Normal(_) => Some(component),
            }
        })
        .collect::<PathBuf>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_join() {
        let init_path = PathBuf::from("/abc/efg");
        assert_eq!(init_path.secure_join("hyc"), PathBuf::from("/abc/efg/hyc"));
        assert_eq!(
            init_path.secure_join("hyc/hop"),
            PathBuf::from("/abc/efg/hyc/hop")
        );
        assert_eq!(
            init_path.secure_join("../attack"),
            PathBuf::from("/abc/efg/attack")
        );
        assert_eq!(
            init_path.secure_join("attack/.."),
            PathBuf::from("/abc/efg/attack")
        );
        assert_eq!(
            init_path.secure_join("/attack"),
            PathBuf::from("/abc/efg/attack")
        );
        assert_eq!(
            init_path.secure_join("./attack"),
            PathBuf::from("/abc/efg/attack")
        );
        assert_eq!(
            init_path.secure_join("attack/."),
            PathBuf::from("/abc/efg/attack")
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_remove_verbatim_prefix() {
        let path = Path::new(r"c:\windows\System32")
            .to_path_buf()
            .canonicalize()
            .expect("should canonicalize: c:\\");

        assert_eq!(
            PathBuf::from(r"C:\Windows\System32"),
            normalize_path(path).unwrap()
        );
    }
}
