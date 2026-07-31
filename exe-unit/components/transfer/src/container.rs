use cap_fs_ext::{OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_primitives::ambient_authority;
use cap_primitives::fs::{
    self as cap_fs, DirOptions, FollowSymlinks, OpenOptions as CapOpenOptions,
};
use futures::future::LocalBoxFuture;
use futures::{FutureExt, SinkExt, StreamExt};
use percent_encoding::percent_decode;
use std::convert::TryFrom;
use std::ffi::OsStr;
use std::fs::File as StdFile;
use std::io::{self, Seek, SeekFrom as StdSeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, SeekFrom};
use tokio::task::spawn_local;
use url::Url;

use crate::archive::{archive, extract, ArchiveFormat};
use crate::error::Error;
use crate::file::DEFAULT_CHUNK_SIZE;
use crate::{abortable_sink, abortable_stream, PathTraverse};
use crate::{TransferContext, TransferData, TransferProvider, TransferSink, TransferStream};

use ya_runtime_api::deploy::ContainerVolume;

pub struct ContainerTransferProvider {
    work_dir: PathBuf,
    vols: Vec<ContainerVolume>,
}

#[derive(Clone, Debug)]
struct ContainerLocation {
    volume_name: PathBuf,
    relative_path: PathBuf,
}

fn invalid_path(path: impl std::fmt::Display) -> Error {
    Error::IoError(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("unsafe container path: [{path}]"),
    ))
}

fn decode_path(url: &Url) -> Result<String, Error> {
    percent_decode(url.path().as_bytes())
        .decode_utf8()
        .map(|path| path.into_owned())
        .map_err(|_| invalid_path(url.path()))
}

fn validate_container_path(path: &str) -> Result<(), Error> {
    if !path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Err(invalid_path(path));
    }

    let mut components = path.split('/');
    if components.next() != Some("") {
        return Err(invalid_path(path));
    }

    let mut has_component = false;
    for component in components {
        if component.is_empty() || component == "." || component == ".." || component.contains(':')
        {
            return Err(invalid_path(path));
        }
        has_component = true;
    }

    if !has_component {
        return Err(invalid_path(path));
    }
    Ok(())
}

fn validate_relative_path(path: &Path, allow_empty: bool) -> Result<(), Error> {
    if !allow_empty && path.as_os_str().is_empty() {
        return Err(invalid_path(path.display()));
    }
    for component in path.components() {
        match component {
            Component::Normal(name)
                if !name.is_empty() && name != OsStr::new(".") && name != OsStr::new("..") => {}
            _ => return Err(invalid_path(path.display())),
        }
    }
    Ok(())
}

fn validate_volume_name(path: &Path) -> Result<(), Error> {
    if path == Path::new(".") {
        return Ok(());
    }
    validate_relative_path(path, false)
}

fn open_dir_components(root: &StdFile, path: &Path, create: bool) -> io::Result<StdFile> {
    let mut current = root.try_clone()?;
    for component in path.components() {
        let name = match component {
            Component::Normal(name) => name,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsafe path component",
                ))
            }
        };

        current = match cap_fs::open_dir_nofollow(&current, Path::new(name)) {
            Ok(dir) => dir,
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                match cap_fs::create_dir(&current, Path::new(name), &DirOptions::new()) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                cap_fs::open_dir_nofollow(&current, Path::new(name))?
            }
            Err(error) => return Err(error),
        };
    }
    Ok(current)
}

fn split_file_path(path: &Path) -> Result<(&Path, &OsStr), Error> {
    let name = path
        .file_name()
        .ok_or_else(|| invalid_path(path.display()))?;
    Ok((path.parent().unwrap_or_else(|| Path::new("")), name))
}

fn cap_read_options() -> CapOpenOptions {
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    options
}

fn cap_write_options(truncate: bool) -> CapOpenOptions {
    let mut options = CapOpenOptions::new();
    options
        .write(true)
        .create(true)
        .truncate(truncate)
        .follow(FollowSymlinks::No)
        .nonblock(true);
    options
}

fn ensure_regular(file: &StdFile) -> io::Result<()> {
    if file.metadata()?.is_file() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "container transfers only allow regular files",
        ))
    }
}

fn copy_cap_tree(source: &StdFile, destination: &StdFile) -> io::Result<()> {
    for entry in cap_fs::read_base_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let entry_type = entry.file_type()?;

        if entry_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("symlink rejected: {}", name.to_string_lossy()),
            ));
        }

        if entry_type.is_dir() {
            let source_child = cap_fs::open_dir_nofollow(source, Path::new(&name))?;
            match cap_fs::create_dir(destination, Path::new(&name), &DirOptions::new()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            let destination_child = cap_fs::open_dir_nofollow(destination, Path::new(&name))?;
            copy_cap_tree(&source_child, &destination_child)?;
        } else if entry_type.is_file() {
            let mut source_file = cap_fs::open(source, Path::new(&name), &cap_read_options())?;
            ensure_regular(&source_file)?;

            let mut destination_file =
                cap_fs::open(destination, Path::new(&name), &cap_write_options(true))?;
            ensure_regular(&destination_file)?;
            io::copy(&mut source_file, &mut destination_file)?;
            destination_file.flush()?;
            destination_file.sync_all()?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "special filesystem object rejected: {}",
                    name.to_string_lossy()
                ),
            ));
        }
    }
    Ok(())
}

impl ContainerTransferProvider {
    pub fn new(work_dir: PathBuf, vols: Vec<ContainerVolume>) -> Self {
        ContainerTransferProvider { work_dir, vols }
    }

    fn resolve_location(&self, container_path: &str) -> Result<ContainerLocation, Error> {
        validate_container_path(container_path)?;

        fn is_prefix_of(base: &str, path: &str) -> usize {
            if base.is_empty() {
                1
            } else if path.starts_with(base)
                && (path == base || path[base.len()..].starts_with('/'))
            {
                base.len() + 1
            } else {
                0
            }
        }

        if let Some((_, c)) = self
            .vols
            .iter()
            .map(|c| (is_prefix_of(&c.path, container_path), c))
            .max_by_key(|(prefix, _)| *prefix)
            .filter(|(prefix, _)| (*prefix) > 0)
        {
            let relative_path = if c.path == container_path {
                PathBuf::new()
            } else {
                PathBuf::from(&container_path[c.path.len() + 1..])
            };
            validate_relative_path(&relative_path, true)?;
            validate_volume_name(Path::new(&c.name))?;

            Ok(ContainerLocation {
                volume_name: PathBuf::from(&c.name),
                relative_path,
            })
        } else {
            log::warn!("path not found in container: {}", container_path);
            Err(Error::IoError(io::Error::new(
                io::ErrorKind::NotFound,
                anyhow::anyhow!("path not found in container: {}", container_path),
            )))
        }
    }

    fn resolve_url(&self, url: &Url) -> Result<ContainerLocation, Error> {
        self.resolve_location(&decode_path(url)?)
    }

    #[cfg(test)]
    fn resolve_path(&self, container_path: &str) -> Result<PathBuf, Error> {
        let location = self.resolve_location(container_path)?;
        Ok(self
            .work_dir
            .join(location.volume_name)
            .join(location.relative_path))
    }

    fn open_work_dir(&self) -> Result<StdFile, Error> {
        Ok(cap_fs::open_ambient_dir(
            &self.work_dir,
            ambient_authority(),
        )?)
    }

    fn open_volume(&self, location: &ContainerLocation, create: bool) -> Result<StdFile, Error> {
        let work_dir = self.open_work_dir()?;
        if location.volume_name == Path::new(".") {
            return Ok(work_dir);
        }
        Ok(open_dir_components(
            &work_dir,
            &location.volume_name,
            create,
        )?)
    }

    fn open_source_file(&self, location: &ContainerLocation) -> Result<StdFile, Error> {
        validate_relative_path(&location.relative_path, false)?;
        let volume = self.open_volume(location, false)?;
        let (parent, name) = split_file_path(&location.relative_path)?;
        let parent = open_dir_components(&volume, parent, false)?;
        let file = cap_fs::open(&parent, Path::new(name), &cap_read_options())?;
        ensure_regular(&file)?;
        Ok(file)
    }

    fn open_destination_file(
        &self,
        location: &ContainerLocation,
        truncate: bool,
    ) -> Result<StdFile, Error> {
        validate_relative_path(&location.relative_path, false)?;
        let volume = self.open_volume(location, true)?;
        let (parent, name) = split_file_path(&location.relative_path)?;
        let parent = open_dir_components(&volume, parent, true)?;
        let file = cap_fs::open(&parent, Path::new(name), &cap_write_options(truncate))?;
        ensure_regular(&file)?;
        Ok(file)
    }

    fn open_source_dir(&self, location: &ContainerLocation) -> Result<StdFile, Error> {
        let volume = self.open_volume(location, false)?;
        Ok(open_dir_components(
            &volume,
            &location.relative_path,
            false,
        )?)
    }

    fn open_destination_dir(&self, location: &ContainerLocation) -> Result<StdFile, Error> {
        let volume = self.open_volume(location, true)?;
        Ok(open_dir_components(&volume, &location.relative_path, true)?)
    }
}

impl TransferProvider<TransferData, Error> for ContainerTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["container"]
    }

    fn source(&self, url: &Url, ctx: &TransferContext) -> TransferStream<TransferData, Error> {
        let location = match self.resolve_url(url) {
            Ok(location) => location,
            Err(error) => return TransferStream::err(error),
        };
        if ctx.args.format.is_some() {
            let source_dir = match self.open_source_dir(&location) {
                Ok(dir) => dir,
                Err(error) => return TransferStream::err(error),
            };
            let args = ctx.args.clone();
            let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
            let mut txc = tx.clone();

            spawn_local(async move {
                let fut = async move {
                    let staging = tempdir::TempDir::new("container-transfer-source")?;
                    let staging_dir =
                        cap_fs::open_ambient_dir(staging.path(), ambient_authority())?;
                    tokio::task::spawn_blocking(move || copy_cap_tree(&source_dir, &staging_dir))
                        .await
                        .map_err(|error| Error::Other(error.to_string()))??;

                    let format = ArchiveFormat::try_from(&args)?;
                    let path_iter = args.traverse(staging.path())?;
                    let (evt_tx, mut evt_rx) = futures::channel::mpsc::channel(1);
                    spawn_local(async move {
                        while let Some(event) = evt_rx.next().await {
                            log::debug!("Container compress: {:?}", event);
                        }
                    });
                    let mut inner =
                        archive(path_iter, staging.path().to_path_buf(), format, evt_tx).await;
                    while let Some(item) = inner.next().await {
                        txc.send(item).await?;
                    }
                    Ok(())
                };
                abortable_stream(fut, abort_reg, tx).await
            });

            return stream;
        }

        let file = match self.open_source_file(&location) {
            Ok(file) => file,
            Err(error) => return TransferStream::err(error),
        };
        let state = ctx.state.clone();
        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let mut txc = tx.clone();

        spawn_local(async move {
            let fut = async move {
                let mut file = File::from_std(file);
                let metadata = file.metadata().await?;
                state.set_size(Some(metadata.len()));
                file.seek(SeekFrom::Start(state.offset())).await?;

                let mut reader = BufReader::with_capacity(DEFAULT_CHUNK_SIZE, file);
                let mut buf = [0u8; DEFAULT_CHUNK_SIZE];
                let mut remaining = metadata.len().saturating_sub(state.offset());
                while remaining > 0 {
                    let count = std::cmp::min(remaining as usize, DEFAULT_CHUNK_SIZE);
                    reader.read_exact(&mut buf[..count]).await?;
                    txc.send(Ok(TransferData::from(buf[..count].to_vec())))
                        .await?;
                    remaining -= count as u64;
                }
                Ok(())
            };
            abortable_stream(fut, abort_reg, tx).await
        });

        stream
    }

    fn destination(&self, url: &Url, ctx: &TransferContext) -> TransferSink<TransferData, Error> {
        let location = match self.resolve_url(url) {
            Ok(location) => location,
            Err(error) => return TransferSink::err(error),
        };
        if ctx.args.format.is_some() {
            let destination_dir = match self.open_destination_dir(&location) {
                Ok(dir) => dir,
                Err(error) => return TransferSink::err(error),
            };
            let args = ctx.args.clone();
            let (sink, rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

            spawn_local(async move {
                let fut = async move {
                    let staging = tempdir::TempDir::new("container-transfer-destination")?;
                    let format = ArchiveFormat::try_from(&args)?;
                    let (evt_tx, mut evt_rx) = futures::channel::mpsc::channel(1);
                    spawn_local(async move {
                        while let Some(event) = evt_rx.next().await {
                            log::debug!("Container extract: {:?}", event);
                        }
                    });
                    extract(rx, staging.path().to_path_buf(), format, evt_tx).await?;

                    let staging_dir =
                        cap_fs::open_ambient_dir(staging.path(), ambient_authority())?;
                    tokio::task::spawn_blocking(move || {
                        copy_cap_tree(&staging_dir, &destination_dir)
                    })
                    .await
                    .map_err(|error| Error::Other(error.to_string()))??;
                    Ok(())
                };
                abortable_sink(fut, res_tx).await
            });
            return sink;
        }

        let offset = ctx.state.offset();
        let mut file = match self.open_destination_file(&location, offset == 0) {
            Ok(file) => file,
            Err(error) => return TransferSink::err(error),
        };
        if offset > 0 {
            if let Err(error) = file.seek(StdSeekFrom::Start(offset)) {
                return TransferSink::err(error.into());
            }
        }

        let state = ctx.state.clone();
        let (sink, mut rx, res_tx) = TransferSink::<TransferData, Error>::create(1);
        spawn_local(async move {
            let fut = async move {
                let mut file = File::from_std(file);
                while let Some(result) = rx.next().await {
                    let data = result?;
                    let bytes = data.as_ref();
                    if bytes.is_empty() {
                        break;
                    }
                    file.write_all(bytes).await?;
                    state.set_offset(state.offset() + bytes.len() as u64);
                }
                file.flush().await?;
                file.sync_all().await?;
                Ok(())
            };
            abortable_sink(fut, res_tx).await
        });
        sink
    }

    fn prepare_destination<'a>(
        &self,
        url: &Url,
        ctx: &TransferContext,
    ) -> LocalBoxFuture<'a, Result<(), Error>> {
        if ctx.args.format.is_some() {
            ctx.state.set_offset(0);
            return futures::future::ok(()).boxed_local();
        }

        let result = (|| {
            let location = self.resolve_url(url)?;
            let volume = self.open_volume(&location, true)?;
            let (parent, name) = split_file_path(&location.relative_path)?;
            let parent = match open_dir_components(&volume, parent, false) {
                Ok(parent) => parent,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    ctx.state.set_offset(0);
                    return Ok(());
                }
                Err(error) => return Err(Error::from(error)),
            };
            match cap_fs::stat(&parent, Path::new(name), FollowSymlinks::No) {
                Ok(metadata) if metadata.is_file() => ctx.state.set_offset(metadata.len()),
                Ok(_) => return Err(invalid_path(location.relative_path.display())),
                Err(error) if error.kind() == io::ErrorKind::NotFound => ctx.state.set_offset(0),
                Err(error) => return Err(error.into()),
            }
            Ok(())
        })();
        futures::future::ready(result).boxed_local()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_resolve_1() {
        let c = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![
                ContainerVolume {
                    name: "vol-3a9710d2-42f1-4502-9098-bc0bab9e7acc".into(),
                    path: "/in".into(),
                },
                ContainerVolume {
                    name: "vol-17599e4b-3aab-4fa8-b08d-440f48bd61e9".into(),
                    path: "/out".into(),
                },
            ],
        );
        assert_eq!(
            c.resolve_path("/in/task.json").unwrap(),
            std::path::Path::new("/tmp/vol-3a9710d2-42f1-4502-9098-bc0bab9e7acc/task.json")
        );
        assert_eq!(
            c.resolve_path("/out/task.json").unwrap(),
            std::path::Path::new("/tmp/vol-17599e4b-3aab-4fa8-b08d-440f48bd61e9/task.json")
        );
        assert!(c.resolve_path("/outs/task.json").is_err());
        assert!(c.resolve_path("/in//task.json").is_err());
        assert_eq!(
            c.resolve_path("/in").unwrap(),
            std::path::Path::new("/tmp/vol-3a9710d2-42f1-4502-9098-bc0bab9e7acc")
        );
    }

    #[test]
    fn test_resolve_2() {
        let c = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![
                ContainerVolume {
                    name: "vol-1".into(),
                    path: "/in/dst".into(),
                },
                ContainerVolume {
                    name: "vol-2".into(),
                    path: "/in".into(),
                },
                ContainerVolume {
                    name: "vol-3".into(),
                    path: "/out".into(),
                },
                ContainerVolume {
                    name: "vol-4".into(),
                    path: "/out/bin".into(),
                },
                ContainerVolume {
                    name: "vol-5".into(),
                    path: "/out/lib".into(),
                },
            ],
        );

        let check_resolve = |container_path, expected_result| {
            assert_eq!(
                c.resolve_path(container_path).unwrap(),
                Path::new(expected_result)
            )
        };

        check_resolve("/in/task.json", "/tmp/vol-2/task.json");
        check_resolve("/in/dst/smok.bin", "/tmp/vol-1/smok.bin");
        check_resolve("/out/b/x.png", "/tmp/vol-3/b/x.png");
        check_resolve("/out/bin/bash", "/tmp/vol-4/bash");
        check_resolve("/out/lib/libc.so", "/tmp/vol-5/libc.so");
    }

    // [ContainerVolume { name: "", path: "" }, ContainerVolume { name: "", path: "" }, ContainerVo
    //        │ lume { name: "", path: "" }]
    #[test]
    fn test_resolve_3() {
        let c = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![
                ContainerVolume {
                    name: "vol-bd959639-9148-4d7c-8ba2-05a654e84476".into(),
                    path: "/golem/output".into(),
                },
                ContainerVolume {
                    name: "vol-4d59d1d6-2571-4ab8-a86a-b6199a9a1f4b".into(),
                    path: "/golem/resource".into(),
                },
                ContainerVolume {
                    name: "vol-b51194da-2fce-45b7-bff8-37e4ef8f7535".into(),
                    path: "/golem/work".into(),
                },
            ],
        );

        let check_resolve = |container_path, expected_result| {
            assert_eq!(
                c.resolve_path(container_path).unwrap(),
                Path::new(expected_result)
            )
        };

        check_resolve(
            "/golem/resource/scene.blend",
            "/tmp/vol-4d59d1d6-2571-4ab8-a86a-b6199a9a1f4b/scene.blend",
        );
    }

    #[test]
    fn test_resolve_compat() {
        let c = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![ContainerVolume {
                name: ".".into(),
                path: "".into(),
            }],
        );
        eprintln!("{}", c.resolve_path("/in/tasks.json").unwrap().display());
    }

    #[test]
    fn rejects_unsafe_decoded_paths() {
        let provider = ContainerTransferProvider::new(
            "/tmp".into(),
            vec![ContainerVolume {
                name: "vol-1".into(),
                path: "/input".into(),
            }],
        );

        for path in [
            "/input/../secret",
            "/input/./secret",
            "/input//secret",
            "/input/back\\slash",
            "/input/file:stream",
        ] {
            assert!(
                provider.resolve_location(path).is_err(),
                "{} should be rejected",
                path
            );
        }

        let encoded = Url::parse("container:/input/..%2f..%2fsecret").expect("valid encoded URL");
        assert!(provider.resolve_url(&encoded).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_for_files_and_directories() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempdir::TempDir::new("container-symlink-test")?;
        let work_dir = temp.path().join("work");
        let volume_dir = work_dir.join("vol-1");
        let outside_dir = temp.path().join("outside");
        std::fs::create_dir_all(&volume_dir)?;
        std::fs::create_dir_all(&outside_dir)?;
        std::fs::write(outside_dir.join("secret"), b"secret")?;
        std::fs::write(outside_dir.join("destination"), b"unchanged")?;

        symlink(&outside_dir, volume_dir.join("linked-dir"))?;
        symlink(
            outside_dir.join("destination"),
            volume_dir.join("linked-file"),
        )?;

        let provider = ContainerTransferProvider::new(
            work_dir,
            vec![ContainerVolume {
                name: "vol-1".into(),
                path: "/input".into(),
            }],
        );

        let source = provider.resolve_location("/input/linked-dir/secret")?;
        assert!(provider.open_source_file(&source).is_err());

        let destination = provider.resolve_location("/input/linked-file")?;
        assert!(provider.open_destination_file(&destination, true).is_err());
        assert_eq!(
            std::fs::read(outside_dir.join("destination"))?,
            b"unchanged"
        );

        let source_dir = provider.resolve_location("/input")?;
        let source_dir = provider.open_source_dir(&source_dir)?;
        let staging = tempdir::TempDir::new("container-symlink-staging")?;
        let staging_dir = cap_fs::open_ambient_dir(staging.path(), ambient_authority())?;
        assert!(copy_cap_tree(&source_dir, &staging_dir).is_err());

        Ok(())
    }

    fn volume_location(name: &str) -> ContainerLocation {
        ContainerLocation {
            volume_name: PathBuf::from(name),
            relative_path: PathBuf::new(),
        }
    }

    /// A symlinked volume root is never followed, so it can neither escape `work_dir`
    /// nor redirect one volume's traffic into another. Plain directories still resolve.
    #[cfg(unix)]
    #[test]
    fn volume_root_symlink_is_not_followed() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempdir::TempDir::new("volume-root-escape")?;
        let work_dir = temp.path().join("work");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&work_dir)?;
        std::fs::create_dir_all(&outside)?;
        std::fs::write(outside.join("secret"), b"secret")?;

        symlink(&outside, work_dir.join("vol-absolute"))?;
        symlink("../outside", work_dir.join("vol-relative"))?;
        // A symlink whose target stays inside the work dir is refused too: it cannot
        // escape, but following it would let one volume redirect into another.
        std::fs::create_dir_all(work_dir.join("vol-target"))?;
        symlink("vol-target", work_dir.join("vol-inside"))?;
        std::fs::create_dir_all(work_dir.join("vol-plain"))?;

        let provider = ContainerTransferProvider::new(work_dir, Vec::new());

        for name in ["vol-absolute", "vol-relative", "vol-inside"] {
            for create in [false, true] {
                assert!(
                    provider
                        .open_volume(&volume_location(name), create)
                        .is_err(),
                    "symlinked volume root {} (create={}) must not resolve",
                    name,
                    create
                );
            }
        }
        assert!(provider
            .open_volume(&volume_location("vol-plain"), false)
            .is_ok());
        assert_eq!(std::fs::read(outside.join("secret"))?, b"secret");

        Ok(())
    }

    /// Symlinks are not followed for any component of a multi-segment volume name.
    #[cfg(unix)]
    #[test]
    fn volume_name_intermediate_components_are_not_followed() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempdir::TempDir::new("volume-intermediate")?;
        let work_dir = temp.path().join("work");
        std::fs::create_dir_all(work_dir.join("real/inner"))?;
        symlink("real", work_dir.join("linked"))?;

        let provider = ContainerTransferProvider::new(work_dir, Vec::new());
        assert!(provider
            .open_volume(&volume_location("real/inner"), false)
            .is_ok());
        assert!(provider
            .open_volume(&volume_location("linked/inner"), false)
            .is_err());

        Ok(())
    }

    /// A multi-segment volume name must still have its intermediate directories created;
    /// `cap_fs::create_dir` alone is `mkdir`, not `mkdir -p`.
    #[test]
    fn nested_volume_name_creates_intermediate_dirs() -> anyhow::Result<()> {
        let temp = tempdir::TempDir::new("volume-nested-create")?;
        let work_dir = temp.path().join("work");
        std::fs::create_dir_all(&work_dir)?;

        let provider = ContainerTransferProvider::new(work_dir.clone(), Vec::new());
        assert!(provider
            .open_volume(&volume_location("outer/inner"), true)
            .is_ok());
        assert!(work_dir.join("outer").join("inner").is_dir());

        Ok(())
    }
}
