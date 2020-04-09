use std::path::{Component, Path, PathBuf};

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
