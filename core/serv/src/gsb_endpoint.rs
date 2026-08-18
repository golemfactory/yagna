use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use url::Url;

#[cfg(unix)]
use anyhow::anyhow;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};

#[cfg(unix)]
const MAX_UNIX_SOCKET_PATH_BYTES: usize = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedGsbEndpoint {
    url: Url,
    managed: Option<ManagedUnixEndpoint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ManagedUnixEndpoint {
    socket_path: PathBuf,
    trusted_root: PathBuf,
    private_dirs: Vec<PathBuf>,
}

impl ResolvedGsbEndpoint {
    pub fn url(&self) -> &Url {
        &self.url
    }

    #[cfg(all(test, unix))]
    pub fn managed_socket_path(&self) -> Option<&Path> {
        self.managed
            .as_ref()
            .map(|managed| managed.socket_path.as_path())
    }

    #[cfg(test)]
    pub fn is_managed_default(&self) -> bool {
        self.managed.is_some()
    }
}

/// Resolves GSB after the data directory. Explicit URLs are returned unchanged.
pub fn resolve(
    explicit: Option<Url>,
    data_dir: &Path,
    is_default_data_dir: bool,
) -> Result<ResolvedGsbEndpoint> {
    if let Some(url) = explicit {
        return Ok(ResolvedGsbEndpoint { url, managed: None });
    }

    resolve_generated(ya_sb_proto::GeneratedGsbEndpoint::for_data_dir(
        data_dir,
        is_default_data_dir,
    ))
}

fn resolve_generated(endpoint: ya_sb_proto::GeneratedGsbEndpoint) -> Result<ResolvedGsbEndpoint> {
    match endpoint {
        ya_sb_proto::GeneratedGsbEndpoint::Tcp(addr) => Ok(ResolvedGsbEndpoint {
            url: Url::parse(&format!("tcp://{addr}"))
                .context("invalid generated GSB TCP endpoint")?,
            managed: None,
        }),
        ya_sb_proto::GeneratedGsbEndpoint::Unix(endpoint) => resolve_generated_unix(endpoint),
    }
}

#[cfg(unix)]
fn resolve_generated_unix(
    endpoint: ya_sb_proto::DefaultUnixGsbEndpoint,
) -> Result<ResolvedGsbEndpoint> {
    managed_endpoint(
        endpoint.socket_path,
        endpoint.trusted_root,
        endpoint.private_dirs,
    )
}

#[cfg(not(unix))]
fn resolve_generated_unix(
    _endpoint: ya_sb_proto::DefaultUnixGsbEndpoint,
) -> Result<ResolvedGsbEndpoint> {
    bail!("generated Unix GSB endpoint is unsupported on this platform")
}

#[cfg(unix)]
fn managed_endpoint(
    socket_path: PathBuf,
    trusted_root: PathBuf,
    private_dirs: Vec<PathBuf>,
) -> Result<ResolvedGsbEndpoint> {
    validate_socket_path(&socket_path)?;
    let file_url = Url::from_file_path(&socket_path).map_err(|_| {
        anyhow!(
            "cannot encode GSB socket path {} as a URL; set --gsb-url explicitly",
            socket_path.display()
        )
    })?;
    let url = Url::parse(&format!("unix:{}", file_url.path()))
        .context("cannot encode managed GSB socket URL")?;
    Ok(ResolvedGsbEndpoint {
        url,
        managed: Some(ManagedUnixEndpoint {
            socket_path,
            trusted_root,
            private_dirs,
        }),
    })
}

#[cfg(unix)]
fn validate_socket_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!(
            "managed GSB socket path {} is not absolute; set --gsb-url explicitly",
            path.display()
        );
    }
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) {
        bail!("managed GSB socket path contains NUL; set --gsb-url explicitly");
    }
    if bytes.len() > MAX_UNIX_SOCKET_PATH_BYTES {
        bail!(
            "managed GSB socket path {} is {} bytes (maximum {}); use --gsb-url with a shorter path",
            path.display(),
            bytes.len(),
            MAX_UNIX_SOCKET_PATH_BYTES
        );
    }
    Ok(())
}

/// Prepares only generated defaults. Explicit URLs are left untouched.
pub fn prepare_private_default_parent(endpoint: &ResolvedGsbEndpoint) -> Result<bool> {
    let Some(managed) = endpoint.managed.as_ref() else {
        return Ok(false);
    };

    #[cfg(unix)]
    {
        let uid = unsafe { libc::geteuid() };
        validate_owned_dir(&managed.trusted_root, uid)?;
        for dir in &managed.private_dirs {
            ensure_private_dir(dir, uid)?;
        }
        Ok(true)
    }

    #[cfg(not(unix))]
    {
        let _ = managed;
        Ok(false)
    }
}

/// Sets `0600` after bind, only for a generated Unix default.
pub fn finalize_private_default_socket(endpoint: &ResolvedGsbEndpoint) -> Result<bool> {
    let Some(managed) = endpoint.managed.as_ref() else {
        return Ok(false);
    };

    #[cfg(unix)]
    {
        finalize_socket(&managed.socket_path, unsafe { libc::geteuid() })?;
        Ok(true)
    }

    #[cfg(not(unix))]
    {
        let _ = managed;
        Ok(false)
    }
}

#[cfg(unix)]
fn validate_owned_dir(path: &Path, uid: u32) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect GSB directory {}", path.display()))?;
    if !metadata.file_type().is_dir() {
        bail!(
            "GSB path {} is a symlink or not a directory",
            path.display()
        );
    }
    if metadata.uid() != uid {
        bail!(
            "GSB directory {} is owned by uid {}, expected effective uid {}",
            path.display(),
            metadata.uid(),
            uid
        );
    }
    Ok(metadata)
}

#[cfg(unix)]
fn ensure_private_dir(path: &Path, uid: u32) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            if let Err(error) = builder.create(path) {
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error).with_context(|| {
                        format!("cannot create private GSB directory {}", path.display())
                    });
                }
            }
        }
        Err(error) => return Err(error.into()),
    }
    // Type and ownership must be established before chmod (which follows links).
    validate_owned_dir(path, uid)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("cannot set mode 0700 on {}", path.display()))?;
    let metadata = validate_owned_dir(path, uid)?;
    if metadata.permissions().mode() & 0o777 != 0o700 {
        bail!("GSB directory {} is not mode 0700", path.display());
    }
    Ok(())
}

#[cfg(unix)]
fn finalize_socket(path: &Path, uid: u32) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect GSB socket {}", path.display()))?;
    if !metadata.file_type().is_socket() {
        bail!(
            "GSB path {} is a symlink or not a Unix socket",
            path.display()
        );
    }
    if metadata.uid() != uid {
        bail!(
            "GSB socket {} is owned by uid {}, expected effective uid {}",
            path.display(),
            metadata.uid(),
            uid
        );
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("cannot set mode 0600 on {}", path.display()))?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != uid
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        bail!(
            "GSB socket {} changed while applying permissions",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_endpoint_is_preserved_and_unmanaged() {
        let explicit = Url::parse("tcp://127.0.0.1:17464").unwrap();
        let endpoint = resolve(Some(explicit.clone()), Path::new("ignored"), false).unwrap();

        assert_eq!(endpoint.url(), &explicit);
        assert!(!endpoint.is_managed_default());
        assert!(!prepare_private_default_parent(&endpoint).unwrap());
        assert!(!finalize_private_default_socket(&endpoint).unwrap());
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::os::unix::net::UnixListener;

        fn resolve_unix(endpoint: ya_sb_proto::DefaultUnixGsbEndpoint) -> ResolvedGsbEndpoint {
            resolve_generated(ya_sb_proto::GeneratedGsbEndpoint::Unix(endpoint)).unwrap()
        }

        #[test]
        fn linux_runtime_endpoint_is_private_and_namespaced_for_custom_datadirs() {
            let root = tempfile::tempdir().unwrap();
            let runtime_dir = root.path().join("runtime");
            let default_data_dir = root.path().join("default-data");
            let custom_data_dir = root.path().join("custom-data");

            let default = resolve_unix(ya_sb_proto::linux_default_gsb_endpoint(
                &default_data_dir,
                true,
                Some(&runtime_dir),
            ));
            let custom = resolve_unix(ya_sb_proto::linux_default_gsb_endpoint(
                &custom_data_dir,
                false,
                Some(&runtime_dir),
            ));

            assert_eq!(
                default.managed_socket_path(),
                Some(runtime_dir.join("yagna").join("gsb.sock").as_path())
            );
            assert_ne!(default.managed_socket_path(), custom.managed_socket_path());
            assert!(custom
                .managed_socket_path()
                .unwrap()
                .starts_with(runtime_dir.join("yagna")));
        }

        #[test]
        fn datadir_fallback_creates_private_parent_and_secures_socket() {
            let root = tempfile::tempdir().unwrap();
            let data_dir = root.path().join("data");
            fs::create_dir(&data_dir).unwrap();
            let endpoint = resolve_unix(ya_sb_proto::linux_default_gsb_endpoint(
                &data_dir, true, None,
            ));

            assert!(prepare_private_default_parent(&endpoint).unwrap());
            let run_dir = data_dir.join("run");
            assert_eq!(
                fs::symlink_metadata(&run_dir).unwrap().permissions().mode() & 0o777,
                0o700
            );

            let socket_path = endpoint.managed_socket_path().unwrap();
            let _listener = UnixListener::bind(socket_path).unwrap();
            fs::set_permissions(socket_path, fs::Permissions::from_mode(0o666)).unwrap();
            assert!(finalize_private_default_socket(&endpoint).unwrap());
            assert_eq!(
                fs::symlink_metadata(socket_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        #[test]
        fn private_parent_setup_refuses_symlink() {
            let root = tempfile::tempdir().unwrap();
            let data_dir = root.path().join("data");
            let target = root.path().join("target");
            fs::create_dir(&data_dir).unwrap();
            fs::create_dir(&target).unwrap();
            symlink(&target, data_dir.join("run")).unwrap();
            let endpoint = resolve_unix(ya_sb_proto::linux_default_gsb_endpoint(
                &data_dir, true, None,
            ));

            let error = prepare_private_default_parent(&endpoint).unwrap_err();
            assert!(error.to_string().contains("symlink or not a directory"));
            assert!(fs::symlink_metadata(data_dir.join("run"))
                .unwrap()
                .file_type()
                .is_symlink());
        }

        #[test]
        fn macos_style_endpoints_are_distinct_per_datadir() {
            let root = tempfile::tempdir().unwrap();
            let default = resolve_unix(ya_sb_proto::macos_default_gsb_endpoint(
                Path::new("/data/default"),
                true,
                root.path(),
            ));
            let first = resolve_unix(ya_sb_proto::macos_default_gsb_endpoint(
                Path::new("/data/one"),
                false,
                root.path(),
            ));
            let second = resolve_unix(ya_sb_proto::macos_default_gsb_endpoint(
                Path::new("/data/two"),
                false,
                root.path(),
            ));

            assert_eq!(
                default.managed_socket_path(),
                Some(root.path().join("yagna/gsb.sock").as_path())
            );
            assert_ne!(first.managed_socket_path(), second.managed_socket_path());
        }
    }
}
