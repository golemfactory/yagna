#![allow(dead_code)]
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Stdio;
use std::str::FromStr;
use tokio::process::Command;

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
            if k != "Terminated" {
                in_progress += *v;
            }
        }
        in_progress
    }

    pub fn total_processed(&self) -> u64 {
        self.last1h.get("Terminated").copied().unwrap_or_default()
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
}
