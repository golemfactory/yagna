use anyhow::anyhow;
use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct YagnaMock {
    yagna_dir: PathBuf,
    provider_dir: PathBuf,

    command: YagnaCommand,
    process: Arc<Mutex<Option<tracker::YagnaTracker>>>,
}

#[derive(Default, Clone)]
pub struct YagnaCommand {
    env: HashMap<String, String>,
}

impl YagnaCommand {
    pub fn new() -> YagnaCommand {
        YagnaCommand {
            env: Default::default(),
        }
    }

    pub fn env(mut self, key: impl ToString, value: impl ToString) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn build(&self, binary: &str) -> anyhow::Result<Command> {
        let mut command = Command::cargo_bin(binary)?;
        for (key, val) in &self.env {
            command.env(key, val);
        }
        Ok(command)
    }

    pub fn build_std(&self, binary: &str) -> std::process::Command {
        let mut command = std::process::Command::new(cargo_bin(binary));
        for (key, val) in &self.env {
            command.env(key, val);
        }
        command
    }

    pub async fn service_run(self) {}
}

impl YagnaMock {
    pub fn new(test_dir: &Path) -> Self {
        let yagna = YagnaMock {
            yagna_dir: test_dir.join("yagna"),
            provider_dir: test_dir.join("provider"),
            command: YagnaCommand {
                env: Default::default(),
            },
            process: Arc::new(Mutex::new(None)),
        };

        yagna.set_default_env()
    }

    fn set_default_env(mut self) -> Self {
        let gsb_url = format!("unix://{}", self.yagna_dir.join("gsb.sock").display());

        self.command = self
            .command
            .env("YAGNA_DATADIR", self.yagna_dir.to_string_lossy())
            .env("DATADIR", self.provider_dir.to_string_lossy())
            .env("YAGNA_API_URL", "http://127.0.0.1:10000")
            .env("YA_NET_TYPE", "hybrid")
            .env("YA_NET_BIND_URL", "udp://0.0.0.0:0")
            .env("YA_NET_RELAY_HOST", "yacn2a.dev.golem.network:7477")
            .env("GSB_URL", gsb_url);
        self
    }

    pub fn command(&self) -> Command {
        self.command.build("yagna").unwrap()
    }

    pub async fn service_run(self) -> anyhow::Result<Self> {
        let mut cmd = self.command.build_std("yagna");
        cmd.current_dir(&self.yagna_dir)
            .arg("service")
            .arg("run")
            .stderr(Stdio::null())
            .stdout(Stdio::null());

        #[cfg(target_os = "linux")]
        unsafe {
            use ::nix::libc::{prctl, PR_SET_PDEATHSIG};
            use ::nix::sys::signal::*;
            use std::io;
            use std::os::unix::process::CommandExt;

            cmd.pre_exec(|| {
                ::nix::unistd::setsid().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                let _ = prctl(PR_SET_PDEATHSIG, SIGTERM);
                Ok(())
            });
        }

        let mut tracker = tracker::YagnaTracker::new(&mut cmd, &self.yagna_dir)?;
        tracker.start().await?;

        *self.process.lock().unwrap() = Some(tracker);
        Ok(self)
    }

    pub(crate) async fn tear_down(&self, timeout: std::time::Duration) -> anyhow::Result<()> {
        let process = {
            self.process
                .lock()
                .map_err(|e| anyhow!("{e}"))?
                .as_ref()
                .map(|tracker| tracker.child.clone())
        };
        if let Some(process) = process {
            if process.terminate(timeout).await.is_err() {
                process.kill();
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
mod tracker {
    use anyhow::Context;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tokio::net;

    use ya_utils_process::{ExeUnitExitStatus, ProcessHandle};

    pub struct YagnaTracker {
        socket: net::UnixDatagram,
        pub child: ProcessHandle,
    }

    impl YagnaTracker {
        pub fn new(command: &mut Command, data_dir: &Path) -> anyhow::Result<Self> {
            let path = data_dir.join("golem.service");
            log::debug!("Golem Service notification socket path: {}", path.display());

            if path.exists() {
                let _ = fs::remove_file(&path);
            } else {
                let parent_dir = path.parent().unwrap();
                fs::create_dir_all(parent_dir).context(format!(
                    "Creating directory {} failed.",
                    parent_dir.display()
                ))?;
            }
            let socket = net::UnixDatagram::bind(&path)
                .context(format!("Binding unix socket {} failed.", path.display()))?;
            command.env("NOTIFY_SOCKET", path);

            Ok(YagnaTracker {
                socket,
                child: ProcessHandle::new(command)?,
            })
        }

        pub async fn start(&mut self) -> anyhow::Result<()> {
            let process = self.child.clone();
            tokio::select! {
                r = process.wait_until_finished() => match r {
                    ExeUnitExitStatus::Finished(s)
                    | ExeUnitExitStatus::Aborted(s) => anyhow::bail!("Golem Service exited prematurely with status: {s}"),
                    ExeUnitExitStatus::Error(e) => anyhow::bail!("Failed to start Golem Service: {e}"),
                },
                r = self.wait_for_start_signal() => match r {
                    Ok(_) => (),
                    Err(e) => {
                        log::error!("Killing Golem Service, since wait failed: {}", e);
                        self.child.terminate(std::time::Duration::from_secs(5)).await?;
                        return Err(e);
                    }
                }
            };
            Ok(())
        }

        pub async fn wait_for_start_signal(&mut self) -> anyhow::Result<()> {
            let mut buf = [0u8; 1024];

            loop {
                let (size, _peer) = self
                    .socket
                    .recv_from(&mut buf)
                    .await
                    .context("Receiving from Golem Service unix socket failed")?;
                let data = &buf[..size];
                for chunk in data.split(|&ch| ch == b'\n') {
                    if chunk == b"READY=1" {
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[cfg(not(unix))]
mod tracker {
    use anyhow::Context;
    use std::path::Path;
    use std::process::Command;

    pub struct YagnaTracker {}

    impl YagnaTracker {
        pub fn new(command: &mut Command, data_dir: &Path) -> anyhow::Result<Self> {
            anyhow::bail!("Tracker implemented only for unix systems")
        }

        pub async fn start(&mut self) -> anyhow::Result<()> {
            anyhow::bail!("Tracker implemented only for unix systems")
        }

        pub async fn wait_for_start_signal(&mut self) -> anyhow::Result<()> {
            anyhow::bail!("Tracker implemented only for unix systems")
        }
    }
}
