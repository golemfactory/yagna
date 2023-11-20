use anyhow::anyhow;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use strum_macros::{Display, EnumString, EnumVariantNames, IntoStaticStr};
use ya_client::model::payment::DriverStatusProperty;

use crate::setup::RunConfig;
use tokio::process::{Child, Command};
use ya_core_model::payment::local::{
    InvoiceStats, InvoiceStatusNotes, NetworkName, StatusNotes, StatusResult,
};
use ya_core_model::version::VersionInfo;

pub struct PaymentPlatform {
    pub platform: &'static str,
    pub driver: &'static str,
    pub token: &'static str,
}

pub struct PaymentDriver {
    pub platforms: HashMap<&'static str, PaymentPlatform>,
    pub name: &'static str,
}

lazy_static! {
    pub static ref ERC20_DRIVER: PaymentDriver = {
        let mut erc20 = HashMap::new();
        erc20.insert(
            NetworkName::Mainnet.into(),
            PaymentPlatform {
                platform: "erc20-mainnet-glm",
                driver: "erc20",
                token: "GLM",
            },
        );
        erc20.insert(
            NetworkName::Rinkeby.into(),
            PaymentPlatform {
                platform: "erc20-rinkeby-tglm",
                driver: "erc20",
                token: "tGLM",
            },
        );
        erc20.insert(
            NetworkName::Goerli.into(),
            PaymentPlatform {
                platform: "erc20-goerli-tglm",
                driver: "erc20",
                token: "tGLM",
            },
        );
        erc20.insert(
            NetworkName::Mumbai.into(),
            PaymentPlatform {
                platform: "erc20-mumbai-tglm",
                driver: "erc20",
                token: "tGLM",
            },
        );
        erc20.insert(
            NetworkName::Polygon.into(),
            PaymentPlatform {
                platform: "erc20-polygon-glm",
                driver: "erc20",
                token: "GLM",
            },
        );

        PaymentDriver {
            platforms: erc20,
            name: "erc20",
        }
    };
    pub static ref ERC20NEXT_DRIVER: PaymentDriver = {
        let mut erc20next = HashMap::new();
        erc20next.insert(
            NetworkName::Mainnet.into(),
            PaymentPlatform {
                platform: "erc20next-mainnet-glm",
                driver: "erc20next",
                token: "GLM",
            },
        );
        erc20next.insert(
            NetworkName::Rinkeby.into(),
            PaymentPlatform {
                platform: "erc20next-rinkeby-tglm",
                driver: "erc20next",
                token: "tGLM",
            },
        );
        erc20next.insert(
            NetworkName::Goerli.into(),
            PaymentPlatform {
                platform: "erc20next-goerli-tglm",
                driver: "erc20next",
                token: "tGLM",
            },
        );
        erc20next.insert(
            NetworkName::Mumbai.into(),
            PaymentPlatform {
                platform: "erc20next-mumbai-tglm",
                driver: "erc20next",
                token: "tGLM",
            },
        );
        erc20next.insert(
            NetworkName::Polygon.into(),
            PaymentPlatform {
                platform: "erc20next-polygon-glm",
                driver: "erc20next",
                token: "GLM",
            },
        );

        PaymentDriver {
            platforms: erc20next,
            name: "erc20next",
        }
    };

    // Drivers are searched in order when more than one supports a given network,
    // so erc20next should be preferred over erc20.
    //
    // If ERC20NEXT_DRIVER isn't here, that's because we wish to use ERC20 on master only. This will
    // be re-enabled shortly.
    pub static ref DRIVERS: Vec<&'static PaymentDriver> = vec![&ERC20_DRIVER];
}

impl PaymentDriver {
    pub fn platform(&self, network: &NetworkName) -> anyhow::Result<&PaymentPlatform> {
        let net: &str = network.into();
        self.platforms
            .get(net)
            .ok_or_else(|| anyhow!("Payment driver config for network '{}' not found.", network))
    }

    pub fn status_label(&self, network: &NetworkName) -> String {
        if network == &NetworkName::Mainnet {
            "on-chain".to_string()
        } else {
            network.to_string().to_lowercase()
        }
    }
}

#[derive(
    Clone,
    Debug,
    Deserialize,
    Display,
    EnumVariantNames,
    EnumString,
    Eq,
    Hash,
    IntoStaticStr,
    PartialEq,
    Serialize,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum NetworkGroup {
    Mainnet,
    Testnet,
}

lazy_static! {
    pub static ref NETWORK_GROUP_MAP: HashMap<NetworkGroup, Vec<NetworkName>> = {
        let mut ngm = HashMap::new();
        ngm.insert(
            NetworkGroup::Mainnet,
            vec![NetworkName::Mainnet, NetworkName::Polygon],
        );
        ngm.insert(
            NetworkGroup::Testnet,
            vec![
                NetworkName::Rinkeby,
                NetworkName::Mumbai,
                NetworkName::Goerli,
            ],
        );
        ngm
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
        log::debug!("Running: {:?}", cmd);
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
                "{:?} failed.: Stdout:\n{}\nStderr:\n{}",
                cmd,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    pub async fn default_id(mut self) -> anyhow::Result<Id> {
        self.cmd.args(["--json", "id", "show"]);
        let output: Result<Id, String> = self.run().await?;
        output.map_err(anyhow::Error::msg)
    }

    pub async fn version(mut self) -> anyhow::Result<VersionInfo> {
        self.cmd.args(["--json", "version", "show"]);
        self.run().await
    }

    pub async fn payment_status(
        mut self,
        address: &str,
        network: &NetworkName,
        payment_driver: &PaymentDriver,
    ) -> anyhow::Result<StatusResult> {
        self.cmd.args(["--json", "payment", "status"]);
        self.cmd.args(["--account", address]);

        let payment_platform = payment_driver.platform(network)?;
        self.cmd.args(["--network", &network.to_string()]);
        self.cmd.args(["--driver", payment_platform.driver]);

        self.run().await
    }

    pub async fn payment_driver_status(
        mut self,
        address: Option<&str>,
        network: Option<&NetworkName>,
        payment_driver: Option<&PaymentDriver>,
    ) -> anyhow::Result<Vec<DriverStatusProperty>> {
        self.cmd.args(["--json", "payment", "driver-status"]);
        if let Some(address) = address {
            self.cmd.args(["--account", address]);
        }

        if let Some(network) = network {
            self.cmd.args(["--network", &network.to_string()]);

            if let Some(payment_driver) = payment_driver {
                let payment_platform = payment_driver.platform(network)?;
                self.cmd.args(["--driver", payment_platform.driver]);
            }
        }

        self.run().await
    }

    pub async fn payment_init(
        mut self,
        address: &str,
        network: &NetworkName,
        payment_driver: &PaymentDriver,
    ) -> anyhow::Result<()> {
        self.cmd.args(["--json", "payment", "init", "--receiver"]); // provider is a receiver
        self.cmd.args(["--account", address]);

        let payment_platform = payment_driver.platform(network)?;
        self.cmd.args(["--network", &network.to_string()]);
        self.cmd.args(["--driver", payment_platform.driver]);

        self.run().await
    }

    pub async fn invoice_status(mut self) -> anyhow::Result<InvoiceStats> {
        self.cmd.args(["--json", "payment", "invoice", "status"]);
        self.run().await
    }

    pub async fn activity_status(mut self) -> anyhow::Result<ActivityStatus> {
        self.cmd.args(["--json", "activity", "status"]);
        self.run().await
    }

    pub async fn forward(self, args: Vec<String>) -> anyhow::Result<i32> {
        let mut cmd = self.cmd;
        let output = cmd.arg("--quiet").args(args).status().await?;

        match output.code() {
            Some(c) => Ok(c),
            None => anyhow::bail!("Unknown process exit code"),
        }
    }

    pub async fn service_run(self, run_cfg: &RunConfig) -> anyhow::Result<Child> {
        let mut cmd = self.cmd;

        cmd.args(["service", "run"]);

        if run_cfg.debug {
            cmd.arg("--debug");
        }
        if let Some(log_dir) = &run_cfg.log_dir {
            cmd.arg("--log-dir");
            cmd.arg(log_dir.to_str().unwrap());
        }

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
        let mut child = cmd.kill_on_drop(true).spawn()?;

        tokio::select! {
            r = child.wait() => match r {
                Ok(s) => anyhow::bail!("Golem Service exited prematurely with status: {}", s),
                Err(e) => anyhow::bail!("Failed to start Golem Service: {}", e),
            },
            r = tracker.wait_for_start() => match r {
                Ok(_) => Ok(child),
                Err(e) => {
                    log::error!("Killing Golem Service, since wait failed: {}", e);
                    child.kill().await?;
                    Err(e)
                }
            }
        }
    }
}

#[cfg(unix)]
mod tracker {
    use anyhow::Context;
    use directories::ProjectDirs;
    use std::fs;
    use tokio::net;
    use tokio::process::Command;

    pub struct Tracker {
        socket: net::UnixDatagram,
    }

    impl Tracker {
        pub fn new(command: &mut Command) -> anyhow::Result<Self> {
            let p =
                ProjectDirs::from("", "GolemFactory", "yagna").expect("Cannot determine home dir");

            let path = p
                .runtime_dir()
                .unwrap_or_else(|| p.data_dir())
                .join("golem.service");
            log::debug!("Golem Service notification socket path: {:?}", path);
            if path.exists() {
                let _ = fs::remove_file(&path);
            } else {
                let parent_dir = path.parent().unwrap();
                fs::create_dir_all(parent_dir)
                    .context(format!("Creating directory {:?} failed.", parent_dir))?;
            }
            let socket = net::UnixDatagram::bind(&path)
                .context(format!("Binding unix socket {:?} failed.", path))?;
            command.env("NOTIFY_SOCKET", path);
            Ok(Tracker { socket })
        }

        pub async fn wait_for_start(&mut self) -> anyhow::Result<()> {
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
    use tokio::process::Command;

    pub struct Tracker {}

    impl Tracker {
        pub fn new(_command: &mut Command) -> anyhow::Result<Self> {
            Ok(Tracker {})
        }

        pub async fn wait_for_start(&mut self) -> anyhow::Result<()> {
            crate::utils::wait_for_yagna().await
        }
    }
}
