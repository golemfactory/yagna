use std::process::Stdio;

use anyhow::Context;
use serde::Deserialize;
use tokio::process::{Child, Command};

use ya_core_model::payment::local::NetworkName;
pub use ya_provider::GlobalsState as ProviderConfig;

use crate::setup::RunConfig;

pub struct YaProviderCommand {
    pub(super) cmd: Command,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Preset {
    pub name: String,
    pub exeunit_name: String,
    pub usage_coeffs: UsageDef,
}

#[derive(Deserialize, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct UsageDef {
    #[serde(default)]
    pub cpu: f64,
    #[serde(default)]
    pub initial: f64,
    #[serde(default)]
    pub duration: f64,
}

impl UsageDef {
    pub fn for_runtime(&self, runtime: &RuntimeInfo) -> Vec<(&str, f64)> {
        let mut v = Vec::new();
        v.push(("initial", self.initial));
        if runtime
            .config
            .counters
            .contains_key("golem.usage.duration_sec")
        {
            v.push(("golem.usage.duration_sec", self.duration));
        }
        if runtime.config.counters.contains_key("golem.usage.cpu_sec") {
            v.push(("golem.usage.cpu_sec", self.cpu));
        }
        v
    }
}

#[derive(Deserialize)]
pub struct RuntimeInfo {
    pub name: String,
    pub description: Option<String>,
    pub config: ya_provider::execution::Configuration,
}

impl YaProviderCommand {
    pub async fn get_config(mut self) -> anyhow::Result<ProviderConfig> {
        let output = self
            .cmd
            .args(&["--json", "config", "get"])
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .await
            .context("failed to get ya-provider configuration")?;

        serde_json::from_slice(output.stdout.as_slice()).context("parsing ya-provider config get")
    }

    pub async fn set_config(
        self,
        config: &ProviderConfig,
        network: &NetworkName,
    ) -> anyhow::Result<()> {
        let mut cmd = self.cmd;

        cmd.args(&["--json", "config", "set"]);

        if let Some(node_name) = &config.node_name {
            cmd.arg("--node-name").arg(&node_name);
        }
        if let Some(subnet) = &config.subnet {
            cmd.arg("--subnet").arg(subnet);
        }

        if let Some(account) = &config.account {
            cmd.args(&["--account", &account.to_string()]);
        }
        cmd.args(&["--payment-network", &network.to_string()]);

        log::debug!("executing: {:?}", cmd);

        let output = cmd
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .await
            .context("failed to set ya-provider configuration")?;

        if output.status.success() {
            Ok(())
        } else {
            let output = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("{}", output))
        }
    }

    pub async fn list_presets(self) -> anyhow::Result<Vec<Preset>> {
        let mut cmd = self.cmd;

        let output = cmd
            .args(&["--json", "preset", "list"])
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .await
            .context("failed to get ya-provider presets")?;

        serde_json::from_slice(output.stdout.as_slice()).context("parsing ya-provider preset list")
    }

    pub async fn list_runtimes(self) -> anyhow::Result<Vec<RuntimeInfo>> {
        let mut cmd = self.cmd;

        let output = cmd
            .args(&["--json", "exe-unit", "list"])
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .await
            .context("failed to get ya-provider exe-unit")?;

        serde_json::from_slice(output.stdout.as_slice())
            .context("parsing ya-provider exe-unit list")
    }

    pub async fn create_preset(
        self,
        name: &str,
        exeunit_name: &str,
        usage_coeffs: &[(&str, f64)],
    ) -> anyhow::Result<()> {
        let mut cmd = self.cmd;
        cmd.args(&["preset", "create", "--no-interactive"]);
        preset_command(&mut cmd, name, exeunit_name, usage_coeffs);
        let output = cmd
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .await
            .context("fail to create preset")?;
        if output.status.success() {
            Ok(())
        } else {
            let output = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("{}", output)).with_context(|| format!("create preset {:?}", name))
        }
    }

    pub async fn update_profile(
        mut self,
        name: &str,
        cores: Option<usize>,
        memory: Option<f64>,
        disk: Option<f64>,
    ) -> anyhow::Result<()> {
        let cmd = &mut self.cmd;
        cmd.arg("profile").arg("update").arg(name);
        if let Some(cores) = cores {
            cmd.arg("--cpu-threads").arg(cores.to_string());
        }
        if let Some(memory) = memory {
            cmd.arg("--mem-gib").arg(memory.to_string());
        }
        if let Some(disk) = disk {
            cmd.arg("--storage-gib").arg(disk.to_string());
        }
        self.exec_no_output().await
    }

    pub async fn update_all_presets(
        mut self,
        starting_fee: Option<f64>,
        env_per_sec: Option<f64>,
        cpu_per_sec: Option<f64>,
    ) -> anyhow::Result<()> {
        let cmd = &mut self.cmd;
        cmd.args(&["preset", "update", "--no-interactive"]);
        cmd.arg("--pricing").arg("linear");
        if let Some(cpu) = cpu_per_sec {
            cmd.arg("--price").arg(format!("CPU={}", cpu));
        }
        if let Some(duration) = env_per_sec {
            cmd.arg("--price").arg(format!("Duration={}", duration));
        }
        if let Some(initial) = starting_fee {
            cmd.arg("--price").arg(format!("Init price={}", initial));
        }
        cmd.arg("--all");
        self.exec_no_output().await
    }

    async fn exec_no_output(self) -> anyhow::Result<()> {
        let mut cmd = self.cmd;
        let output = cmd
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .output()
            .await
            .context("exec ya-provider")?;
        if output.status.success() {
            Ok(())
        } else {
            let output = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("{}", output)).context("exec ya-provider")
        }
    }

    pub async fn update_preset(
        mut self,
        name: &str,
        exeunit_name: &str,
        usage_coeffs: &[(&str, f64)],
    ) -> anyhow::Result<()> {
        let mut cmd = &mut self.cmd;
        cmd.args(&["preset", "update", "--no-interactive"]);
        preset_command(&mut cmd, name, exeunit_name, usage_coeffs);
        cmd.arg("--").arg(name);
        self.exec_no_output()
            .await
            .with_context(|| format!("update preset {}", name))
    }

    pub async fn active_presets(self) -> anyhow::Result<Vec<String>> {
        let mut cmd = self.cmd;
        let output = cmd
            .args(&["--json", "preset", "active"])
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .await
            .context("list active presets")?;
        if output.status.success() {
            serde_json::from_slice(&output.stdout)
                .context("parse ya-provider preset active oputput")
        } else {
            let output = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("{}", output))
        }
    }

    pub async fn set_profile_activity(
        self,
        profile_name: &str,
        activate: bool,
    ) -> anyhow::Result<()> {
        let mut cmd = self.cmd;

        let output = cmd
            .args(&[
                "--json",
                "preset",
                if activate { "activate" } else { "deactivate" },
                profile_name,
            ])
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .stdin(Stdio::null())
            .output()
            .await
            .with_context(|| format!("activating profile {:?}", profile_name))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "{}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .with_context(|| format!("activating profile {:?}", profile_name))?
        }
    }

    pub async fn spawn(mut self, app_key: &str, run_cfg: &RunConfig) -> anyhow::Result<Child> {
        self.cmd
            .args(&[
                "run",
                "--payment-network",
                &run_cfg.account.network.to_string(),
            ])
            .env("YAGNA_APPKEY", app_key);

        if let Some(node_name) = &run_cfg.node_name {
            self.cmd.arg("--node-name").arg(node_name);
        }
        if let Some(subnet) = &run_cfg.subnet {
            self.cmd.arg("--subnet").arg(subnet);
        }

        if let Some(account) = run_cfg.account.account {
            self.cmd.arg("--account").arg(account.to_string());
        }

        if run_cfg.debug {
            self.cmd.arg("--debug");
        }
        if let Some(log_dir) = &run_cfg.log_dir {
            self.cmd.arg("--log-dir");
            self.cmd.arg(log_dir.to_str().unwrap());
        }

        log::debug!("spawning: {:?}", self.cmd);

        Ok(self
            .cmd
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .spawn()?)
    }
}

fn preset_command<'a, 'b>(
    cmd: &mut Command,
    name: impl Into<Option<&'a str>>,
    exeunit_name: impl Into<Option<&'b str>>,
    usage_coeffs: &[(&str, f64)],
) {
    if let Some(name) = name.into() {
        cmd.arg("--preset-name").arg(name);
    }
    if let Some(exeunit_name) = exeunit_name.into() {
        cmd.arg("--exe-unit").arg(exeunit_name);
    }
    cmd.arg("--pricing").arg("linear");
    for &(k, v) in usage_coeffs {
        cmd.arg("--price").arg(format!("{}={}", k, v));
    }
}
