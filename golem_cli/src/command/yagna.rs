#![allow(dead_code)]
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use futures::prelude::*;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Stdio;

use tokio::process::{Child, Command};
use ya_core_model::payment::local::{InvoiceStats, InvoiceStatusNotes, StatusNotes, StatusResult};
use ya_core_model::version::VersionInfo;

pub struct PaymentType {
    pub platform: &'static str,
    pub driver: &'static str,
}

impl PaymentType {
    pub const ZK: PaymentType = PaymentType {
        platform: "zksync-rinkeby-tglm",
        driver: "zksync",
    };
    pub const PLAIN: PaymentType = PaymentType {
        platform: "erc20-rinkeby-tglm",
        driver: "ngnt",
    };
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Id {
    pub node_id: String,
}

pub trait PaymentSummary {
    fn total_pending(&self) -> (BigDecimal, u64);
    fn unconfirmed(&self) -> (BigDecimal, u64);
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
impl PaymentSummary for StatusNotes {
    fn total_pending(&self) -> (BigDecimal, u64) {
        (
            &self.accepted.total_amount - &self.confirmed.total_amount,
            self.accepted.agreements_count - self.confirmed.agreements_count,
        )
    }

    fn unconfirmed(&self) -> (BigDecimal, u64) {
        (
            &self.requested.total_amount - &self.accepted.total_amount,
            self.requested.agreements_count - self.accepted.agreements_count,
        )
    }
}

impl PaymentSummary for InvoiceStatusNotes {
    fn total_pending(&self) -> (BigDecimal, u64) {
        let value = self.accepted.clone();
        (value.total_amount, value.agreements_count)
    }

    fn unconfirmed(&self) -> (BigDecimal, u64) {
        let value = self.issued.clone() + self.received.clone();
        (value.total_amount.clone(), value.agreements_count)
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
            log::trace!("{}", String::from_utf8_lossy(&output.stdout));
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

    pub async fn version(mut self) -> anyhow::Result<VersionInfo> {
        self.cmd.args(&["--json", "version", "show"]);
        self.run().await
    }

    pub async fn payment_status(
        mut self,
        address: Option<&str>,
        payment_type: Option<&PaymentType>,
    ) -> anyhow::Result<StatusResult> {
        self.cmd.args(&["--json", "payment", "status"]);
        if let Some(payment_type) = payment_type {
            self.cmd.arg("-p").arg(payment_type.platform);
        }
        if let Some(addr) = address {
            self.cmd.arg(addr);
        }
        self.run().await
    }

    pub async fn payment_init(
        mut self,
        address: &str,
        payment_type: &PaymentType,
    ) -> anyhow::Result<()> {
        self.cmd.args(&[
            "--json",
            "payment",
            "init",
            "-p",
            "--driver",
            payment_type.driver,
            address,
        ]);
        self.run().await
    }

    pub async fn invoice_status(mut self) -> anyhow::Result<InvoiceStats> {
        self.cmd.args(&["--json", "payment", "invoice", "status"]);
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
        if let Some(core_log) = std::env::var_os("YA_CORE_LOG") {
            cmd.env("RUST_LOG", core_log);
        } else {
            cmd.env("RUST_LOG", "info,actix_web::middleware=warn");
        }

        #[cfg(target_os = "linux")]
        unsafe {
            use ::nix::libc::{prctl, PR_SET_PDEATHSIG};
            use ::nix::sys::signal::*;
            use std::io;

            cmd.pre_exec(|| {
                ::nix::unistd::setsid().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                let _ = prctl(PR_SET_PDEATHSIG, SIGTERM);
                Ok(())
            });
        }

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
                anyhow::bail!("failed to start service");
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
