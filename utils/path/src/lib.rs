use std::ffi::OsString;
use std::io;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

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
}
