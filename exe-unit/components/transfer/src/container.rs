use std::io;
use std::path::PathBuf;

use url::Url;

use crate::error::Error as TransferError;
use crate::location::UrlExt;
use crate::{DirTransferProvider, FileTransferProvider};
use crate::{TransferContext, TransferData, TransferProvider, TransferSink, TransferStream};

use ya_runtime_api::deploy::ContainerVolume;

pub struct ContainerTransferProvider {
    file_tp: FileTransferProvider,
    dir_tp: DirTransferProvider,
    work_dir: PathBuf,
    vols: Vec<ContainerVolume>,
}

impl ContainerTransferProvider {
    pub fn new(work_dir: PathBuf, vols: Vec<ContainerVolume>) -> Self {
        ContainerTransferProvider {
            file_tp: Default::default(),
            dir_tp: Default::default(),
            work_dir,
            vols,
        }
    }

    fn resolve_path(&self, container_path: &str) -> std::result::Result<PathBuf, TransferError> {
        fn is_prefix_of(base: &str, path: &str) -> usize {
            if path.starts_with(base) && (path == base || path[base.len()..].starts_with('/')) {
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
            let vol_base = self.work_dir.join(&c.name);

            if c.path == container_path {
                return Ok(vol_base);
            }

            let path = &container_path[c.path.len() + 1..];
            if path.starts_with('/') {
                return Err(TransferError::IoError(io::Error::new(
                    io::ErrorKind::NotFound,
                    anyhow::anyhow!("invalid path format: [{}]", container_path),
                )));
            }
            Ok(vol_base.join(path))
        } else {
            log::warn!("path not found in container: {}", container_path);
            Err(TransferError::IoError(io::Error::new(
                io::ErrorKind::NotFound,
                anyhow::anyhow!("path not found in container: {}", container_path),
            )))
        }
    }

    fn resolve_url(&self, path: &str) -> std::result::Result<Url, TransferError> {
        Ok(Url::from_file_path(self.resolve_path(path)?).unwrap())
    }
}

impl TransferProvider<TransferData, TransferError> for ContainerTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["container"]
    }

    fn source(
        &self,
        url: &Url,
        ctx: &TransferContext,
    ) -> TransferStream<TransferData, TransferError> {
        let file_url = match self.resolve_url(url.path_decoded().as_str()) {
            Ok(v) => v,
            Err(e) => return TransferStream::err(e),
        };

        if ctx.args.format.is_some() {
            return self.dir_tp.source(&file_url, ctx);
        }
        self.file_tp.source(&file_url, ctx)
    }

    fn destination(
        &self,
        url: &Url,
        ctx: &TransferContext,
    ) -> TransferSink<TransferData, TransferError> {
        let file_url = match self.resolve_url(url.path_decoded().as_str()) {
            Ok(v) => v,
            Err(e) => return TransferSink::err(e),
        };

        if ctx.args.format.is_some() {
            return self.dir_tp.destination(&file_url, ctx);
        }
        self.file_tp.destination(&file_url, ctx)
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
}
