use crate::utils::is_yagna_running;
use anyhow::{bail, Result};

use std::process::Command;

/*async fn get_activity(api: &ProviderApi, id: String) -> Result<Activity> {
    let state = api.activity.get_activity_state(&id).await?;
    let usage = api.activity.get_activity_usage(&id).await?;
    Ok(Activity {
        /*id,*/
        state,
        usage,
    })
}*/

/*async fn activities(api: &ProviderApi) -> Result<()> {
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
}*/

/*async fn payments(api: &ProviderApi) -> Result<()> {
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
}*/

pub async fn run() -> Result</*exit code*/ i32> {
    if !is_yagna_running().await? {
        bail!("Cannot connect to golem!");
    }

    let _ = Command::new("yagna")
        .arg("payment")
        .arg("status")
        .env("RUST_LOG", "error")
        .status()?;

    let _ = Command::new("yagna")
        .arg("activity")
        .arg("status")
        .env("RUST_LOG", "error")
        .status()?;

    Ok(0)
}
