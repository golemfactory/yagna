use crate::{
    appkey::get_app_key,
    utils::{get_command_json_output, is_yagna_running},
};
use anyhow::{bail, Result};
use chrono::{DateTime, Local, TimeZone, Utc};
use futures::stream::{self, StreamExt};
use std::convert::TryFrom;
use structopt::StructOpt;
use ya_client::cli::{ApiOpts, ProviderApi};
use ya_client_model::activity::{ActivityState, ActivityUsage};
use ya_core_model::payment::local::StatusResult as PaymentStatusResult;

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
    let all_jobs = activities.len();
    let jobs_computing = activities
        .iter()
        .filter(|activity| activity.state.alive())
        .count();
    let last_job = activities
        .iter()
        .max_by(|a1, a2| a1.usage.timestamp.cmp(&a2.usage.timestamp));

    println!(
        "computing:\t{}",
        if jobs_computing > 0 {
            format!("yes ({} jobs)", jobs_computing)
        } else {
            "no".to_string()
        }
    );
    println!("jobs computed:\t{}", all_jobs - jobs_computing);
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

async fn get_payment_status(address: &str, platform: &str) -> Result<PaymentStatusResult> {
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

    Ok(serde_json::from_value::<PaymentStatusResult>(output)?)
}

async fn payments(api: &ProviderApi) -> Result<()> {
    let recv_accounts = stream::iter(api.payment.get_accounts().await?)
        .filter_map(|account| async {
            match get_payment_status(&account.address, &account.platform).await {
                Ok(payment_status) => Some((account, payment_status)),
                Err(_) => None,
            }
        })
        .collect::<Vec<_>>()
        .await;

    for (account, payment_status) in recv_accounts {
        println!("wallet:\t{} {}", account.address, account.platform);
        println!("\tamount:\t{}", payment_status.amount);
        println!("\trequested:\t{}", payment_status.incoming.requested);
        println!("\taccepted:\t{}", payment_status.incoming.accepted);
        println!("\tconfirmed:\t{}", payment_status.incoming.confirmed);
    }

    Ok(())
}

pub async fn run() -> Result</*exit code*/ i32> {
    if !is_yagna_running().await? {
        bail!("Cannot connect to golem!");
    }

    let app_key = get_app_key().await?;
    // using ApiOpts::from_iter to have defaults and env variables handled
    let api_opts = ApiOpts::from_iter(&["arg0", "--app-key", &app_key]);
    let api = ProviderApi::try_from(&api_opts)?;

    // TODO: server running

    activities(&api).await?;
    payments(&api).await?;

    // TODO: link to logs

    Ok(0)
}
