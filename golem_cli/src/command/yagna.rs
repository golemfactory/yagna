#![allow(dead_code)]
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use futures::prelude::*;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Stdio;
use std::str::FromStr;
use tokio::process::{Child, Command};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id {
    pub node_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentStatus {
    pub amount: String,
    pub incoming: PaymentSummary,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PaymentSummary {
    #[serde(default)]
    accepted: Option<String>,
    #[serde(default)]
    confirmed: Option<String>,
    #[serde(default)]
    requested: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStatus {
    pub last1h: HashMap<String, u64>,
    pub total: HashMap<String, u64>,
    pub last_activity_ts: Option<DateTime<Utc>>,
}

impl ActivityStatus {
    pub fn last1h_processed(&self) -> u64 {
        self.last1h.get("Terminated").copied().unwrap_or_default()
    }

    pub fn in_progress(&self) -> u64 {
        let mut in_progress = 0;
        for (k, v) in &self.last1h {
            if k != "Terminated" && k != "New" {
                in_progress += *v;
            }
        }
        in_progress
    }

    pub fn total_processed(&self) -> u64 {
        self.total.get("Terminated").copied().unwrap_or_default()
    }
}

impl PaymentSummary {
    pub fn total_pending(&self) -> BigDecimal {
        let accepted = self
            .accepted
            .as_ref()
            .and_then(|v| BigDecimal::from_str(&v).ok())
            .unwrap_or_default();
        let confirmed = self
            .confirmed
            .as_ref()
            .and_then(|v| BigDecimal::from_str(&v).ok())
            .unwrap_or_default();
        //let requested = self.requested.as_ref().and_then(|v| BigDecimal::from_str(&v).ok()).unwrap_or_default();

        accepted + confirmed
    }

    pub fn unconfirmed(&self) -> BigDecimal {
        self.requested
            .as_ref()
            .and_then(|v| BigDecimal::from_str(v).ok())
            .unwrap_or_default()
    }
}

pub struct YagnaCommand {
    pub(super) cmd: Command,
}

impl YagnaCommand {
    async fn run<T: DeserializeOwned>(self) -> anyhow::Result<T> {
        let mut cmd = self.cmd;
        let output = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if output.status.success() {
            Ok(serde_json::from_slice(&output.stdout)?)
        } else {
            Err(anyhow::anyhow!(
                "{}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    pub async fn default_id(mut self) -> anyhow::Result<Id> {
        self.cmd.args(&["--json", "id", "show"]);
        let output: Result<Id, String> = self.run().await?;
        output.map_err(anyhow::Error::msg)
    }

    pub async fn payment_status(mut self) -> anyhow::Result<PaymentStatus> {
        self.cmd.args(&["--json", "payment", "status"]);
        self.run().await
    }

    pub async fn activity_status(mut self) -> anyhow::Result<ActivityStatus> {
        self.cmd.args(&["--json", "activity", "status"]);
        self.run().await
    }

    pub async fn service_run(self) -> anyhow::Result<Child> {
        let mut cmd = self.cmd;

        cmd.args(&["service", "run"]);
        cmd.stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit());

        let mut tracker = tracker::Tracker::new(&mut cmd)?;
        cmd.kill_on_drop(true);
        let child = cmd.spawn()?;
        let wait_for = tracker.wait_for_start();
        futures::pin_mut!(wait_for);
        let v = match future::try_select(child, wait_for).await {
            Ok(v) => Ok(v),
            Err(future::Either::Left((e, _wait))) => Err(e),
            Err(future::Either::Right((e, mut child))) => {
                child.kill()?;
                let _ = child.await?;
                Err(e)
            }
        }?;

        match v {
            future::Either::Left((_child_ends, _t)) => {
                anyhow::bail!("fail to start service");
            }
            future::Either::Right((_t, child)) => Ok(child),
        }
    }
}

#[cfg(unix)]
mod tracker {
    use directories::ProjectDirs;
    use std::{fs, io};
    use tokio::net;
    use tokio::process::Command;

    pub struct Tracker {
        socket: net::UnixDatagram,
    }

    impl Tracker {
        pub fn new(command: &mut Command) -> io::Result<Self> {
            let p = ProjectDirs::from("", "GolemFactory", "yagna").unwrap();

            let path = p
                .runtime_dir()
                .unwrap_or_else(|| p.data_dir())
                .join("golem.service");
            if path.exists() {
                let _ = fs::remove_file(&path).ok();
            } else {
                fs::create_dir_all(path.parent().unwrap())?;
            }
            let socket = net::UnixDatagram::bind(&path)?;
            command.env("NOTIFY_SOCKET", path);
            Ok(Tracker { socket })
        }

        pub async fn wait_for_start(&mut self) -> io::Result<()> {
            let mut buf = [0u8; 1024];

            loop {
                let (size, _peer) = self.socket.recv_from(&mut buf).await?;
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
    use std::io;
    use tokio::process::Command;

    pub struct Tracker {}

    impl Tracker {
        pub fn new(_command: &mut Command) -> io::Result<Self> {
            Ok(Tracker {})
        }

        pub async fn wait_for_start(&mut self) -> io::Result<()> {
            crate::utils::wait_for_yagna()
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }
    }
}
