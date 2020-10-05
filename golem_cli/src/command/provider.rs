use anyhow::Context;

use serde::Deserialize;
use std::process::Stdio;
use tokio::process::Command;

pub struct YaProviderCommand {
    pub(super) cmd: Command,
}

#[derive(Deserialize)]
pub struct ProviderConfig {
    pub node_name: Option<String>,
    pub subnet: Option<String>,
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

    pub async fn set_config(self, config: &ProviderConfig) -> anyhow::Result<()> {
        let mut cmd = self.cmd;

        cmd.args(&["--json", "config", "set"]);

        if let Some(node_name) = &config.node_name {
            cmd.arg("--node-name").arg(&node_name);
        }
        if let Some(subnet) = &config.subnet {
            cmd.arg("--subnet").arg(subnet);
        }

        let output = cmd
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .await
            .context("failed to get ya-provider configuration")?;

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

    pub async fn create_preset(
        self,
        name: &str,
        exeunit_name: &str,
        usage_coeffs: &UsageDef,
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

    pub async fn update_preset(
        self,
        name: &str,
        exeunit_name: &str,
        usage_coeffs: &UsageDef,
    ) -> anyhow::Result<()> {
        let mut cmd = self.cmd;
        cmd.args(&["preset", "update", "--no-interactive"]);
        preset_command(&mut cmd, name, exeunit_name, usage_coeffs);
        cmd.arg("--").arg(name);

        let output = cmd
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .await
            .with_context(|| format!("update preset {:?}", name))?;
        if output.status.success() {
            Ok(())
        } else {
            let output = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("{}", output)).with_context(|| format!("update preset {:?}", name))
        }
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
}

fn preset_command(cmd: &mut Command, name: &str, exeunit_name: &str, usage_coeffs: &UsageDef) {
    cmd.arg("--preset-name").arg(name);
    cmd.arg("--exe-unit").arg(exeunit_name);
    cmd.arg("--pricing").arg("linear");
    cmd.arg("--price").arg(format!("CPU={}", &usage_coeffs.cpu));
    cmd.arg("--price")
        .arg(format!("Duration={}", usage_coeffs.duration));
    cmd.arg("--price")
        .arg(format!("Init price={}", usage_coeffs.initial));
}
