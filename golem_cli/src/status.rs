use anyhow::{anyhow, bail, Result};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Local, TimeZone, Utc};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Deserializer};
use serde_json::json;
use std::convert::TryFrom;
use tokio::process::Command;
use ya_client::cli::{ApiOpts, ProviderApi};
use ya_client_model::activity::{ActivityState, ActivityUsage};

#[derive(Debug)]
struct Activity {
    // id: String,
    state: ActivityState,
    usage: ActivityUsage,
}

async fn get_activity(api: &ProviderApi, id: String) -> Result<Activity> {
    let state = api.activity.get_activity_state(&id).await?;
    let usage = api.activity.get_activity_usage(&id).await?;
    Ok(Activity {
        /*id,*/
        state,
        usage,
    })
}

async fn activities(api: &ProviderApi) -> Result<()> {
    let activities = stream::iter(api.activity.get_activity_ids().await?)
        .filter_map(|id| async { get_activity(&api, id).await.ok() })
        .collect::<Vec<_>>()
        .await;
    let jobs_computed = activities
        .iter()
        .filter(|activity| !activity.state.alive())
        .count();
    let is_job_active = activities.iter().any(|activity| activity.state.alive());
    let last_job = activities
        .iter()
        .max_by(|a1, a2| a1.usage.timestamp.cmp(&a2.usage.timestamp));

    println!(
        "active/idle:\t{}",
        if is_job_active { "active" } else { "idle" }
    );
    println!("jobs computed:\t{}", jobs_computed);
    println!(
        "last job:\t{}",
        match last_job {
            Some(activity) =>
                DateTime::<Local>::from(Utc.timestamp(activity.usage.timestamp, 0)).to_string(),
            None => "Unknown".to_string(),
        }
    );

    Ok(())
}

async fn get_command_json_output(program: &str, args: &[&str]) -> Result<serde_json::Value> {
    let mut command = Command::new(program);
    command.args(args);
    let command_output = command.output().await?;
    if !command_output.status.success() {
        bail!("subcommand failed: {:?}", command);
    }
    Ok(serde_json::from_slice(&command_output.stdout)?)
}

#[derive(Deserialize)]
struct Account {
    platform: String,
    address: String,
    #[allow(dead_code)]
    driver: String,
    #[allow(dead_code)]
    #[serde(deserialize_with = "bool_from_x")]
    send: bool,
    #[serde(deserialize_with = "bool_from_x")]
    recv: bool,
}

fn bool_from_x<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    Ok(s == "X")
}

async fn get_accounts() -> Result<Vec<Account>> {
    let msg_command = "'yagna payment accounts'";
    let mut output = get_command_json_output("yagna", &["payment", "accounts", "--json"]).await?;
    if output.get("headers") != Some(&json!(["platform", "address", "driver", "send", "recv"])) {
        bail!("unexpected output format of {}", msg_command);
    }
    Ok(serde_json::from_value::<Vec<Account>>(
        output
            .get_mut("values")
            .ok_or_else(|| anyhow!("'values' not found in {}", msg_command))?
            .take(),
    )?)
}

#[derive(Deserialize)]
pub struct StatusNotes {
    pub requested: BigDecimal,
    pub accepted: BigDecimal,
    pub confirmed: BigDecimal,
}

#[derive(Deserialize)]
struct PaymentStatus {
    amount: BigDecimal,
    incoming: StatusNotes,
    // outgoing - probably not relevant
    // reserved - likewise
}

async fn get_payment_status(address: &str, platform: &str) -> Result<PaymentStatus> {
    let output = get_command_json_output(
        "yagna",
        &[
            "payment",
            "status",
            "--platform",
            platform,
            address,
            "--json",
        ],
    )
    .await?;

    Ok(serde_json::from_value::<PaymentStatus>(output)?)
}

async fn payments() -> Result<()> {
    let recv_accounts = stream::iter(get_accounts().await?)
        .filter_map(|account| async {
            if !account.recv {
                return None;
            }
            match get_payment_status(&account.address, &account.platform).await {
                Ok(payment_status) => Some((account, payment_status)),
                Err(_) => None,
            }
        })
        .collect::<Vec<_>>()
        .await;

    for (account, payment_status) in recv_accounts {
        println!("wallet:\t{} {}", account.address, account.platform);
        println!("amount:\t{}", payment_status.amount);
        println!("requested:\t{}", payment_status.incoming.requested);
        println!("accepted:\t{}", payment_status.incoming.accepted);
        println!("confirmed:\t{}", payment_status.incoming.confirmed);
    }

    Ok(())
}

pub async fn run(api_opts: &ApiOpts) -> Result</*exit code*/ i32> {
    let api = ProviderApi::try_from(api_opts)?;

    // TODO: server running

    activities(&api).await?;
    payments().await?;

    // TODO: link to logs

    Ok(0)
}
