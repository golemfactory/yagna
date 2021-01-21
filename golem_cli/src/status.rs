use crate::command::{
    PaymentSummary, RecvAccount, YaCommand, DEFAULT_NETWORK, ERC20_DRIVER, ZKSYNC_DRIVER,
};
use crate::platform::Status as KvmStatus;
use crate::utils::is_yagna_running;

use ansi_term::{Colour, Style};
use anyhow::Result;
use futures::prelude::*;
use prettytable::{cell, format, row, Table};
use structopt::StructOpt;
use ya_core_model::payment::local::StatusResult;

#[derive(StructOpt, Debug)]
pub struct StatusCommand {
    #[structopt(long = "payment-network", env = "YA_PAYMENT_NETWORK", default_value = &DEFAULT_NETWORK)]
    pub network: String,
}

async fn payment_status(
    cmd: &YaCommand,
    network: &str,
    account: &Option<RecvAccount>,
) -> anyhow::Result<(StatusResult, StatusResult)> {
    if let Some(account) = account {
        let address = account.address.to_lowercase();
        let (status_zk, status_erc20) = future::join(
            cmd.yagna()?
                .payment_status(Some(&address), network, &ZKSYNC_DRIVER),
            cmd.yagna()?
                .payment_status(Some(&address), network, &ERC20_DRIVER),
        )
        .await;

        let is_zk = account
            .platform
            .as_ref()
            .map(|platform| {
                (*ZKSYNC_DRIVER)
                    .payment_type(Some(network))
                    .map(|desc| desc.platform == platform)
                    .ok()
            })
            .flatten()
            .unwrap_or(true);
        let is_erc20 = account
            .platform
            .as_ref()
            .map(|platform| {
                (*ERC20_DRIVER)
                    .payment_type(Some(network))
                    .map(|desc| desc.platform == platform)
                    .ok()
            })
            .flatten()
            .unwrap_or(true);
        match (status_zk, status_erc20) {
            (Ok(zk), Ok(eth)) => Ok((zk, eth)),
            (Err(e), _) if is_zk => Err(e),
            (_, Err(e)) if is_erc20 => Err(e),
            (Ok(zk), Err(_)) => Ok((zk, StatusResult::default())),
            (Err(_), Ok(plain)) => Ok((StatusResult::default(), plain)),
            (Err(e), _) => Err(e),
        }
    } else {
        let (status_zk, status_erc20) = future::join(
            cmd.yagna()?.payment_status(None, network, &ZKSYNC_DRIVER),
            cmd.yagna()?.payment_status(None, network, &ERC20_DRIVER),
        )
        .await;
        match (status_zk, status_erc20) {
            (Ok(zk), Ok(eth)) => Ok((zk, eth)),
            (Err(e), _) => Err(e),
            (Ok(zk), Err(_)) => Ok((zk, StatusResult::default())),
        }
    }
}

pub async fn run(args: StatusCommand) -> Result</*exit code*/ i32> {
    let size = crossterm::terminal::size().ok().unwrap_or_else(|| (80, 50));
    let cmd = YaCommand::new()?;
    let kvm_status = crate::platform::kvm_status();

    let (config, is_running) =
        future::try_join(cmd.ya_provider()?.get_config(), is_yagna_running()).await?;

    let status = {
        let mut table = Table::new();
        let format = format::FormatBuilder::new().padding(1, 1).build();

        table.set_format(format);
        table.add_row(row![Style::new()
            .fg(Colour::Yellow)
            .underline()
            .paint("Status")]);
        table.add_empty_row();
        if is_running {
            table.add_row(row![
                "Service",
                Style::new().fg(Colour::Green).paint("is running")
            ]);
        } else {
            table.add_row(row![
                "Service",
                Style::new().fg(Colour::Red).paint("is not running")
            ]);
        }
        table.add_row(row!["version", ya_compile_time_utils::semver_str()]);
        table.add_row(row!["commit", ya_compile_time_utils::git_rev()]);
        table.add_row(row!["date", ya_compile_time_utils::build_date()]);
        table.add_row(row![
            "build",
            ya_compile_time_utils::build_number_str().unwrap_or("-")
        ]);

        table.add_empty_row();
        table.add_row(row!["Node Name", &config.node_name.unwrap_or_default()]);
        table.add_row(row!["Subnet", &config.subnet.unwrap_or_default()]);
        if kvm_status.is_implemented() {
            let status = match kvm_status {
                KvmStatus::Valid => Style::new().fg(Colour::Green).paint("valid"),
                KvmStatus::Permission(_) => Style::new().fg(Colour::Red).paint("no access"),
                KvmStatus::NotImplemented => Style::new().paint(""),
                KvmStatus::InvalidEnv(_) => {
                    Style::new().fg(Colour::Red).paint("invalid environment")
                }
            };
            table.add_row(row!["VM", status]);
        }

        table
    };
    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_BOX_CHARS);

    if is_running {
        let payments = {
            let (id, invoice_status) =
                future::try_join(cmd.yagna()?.default_id(), cmd.yagna()?.invoice_status()).await?;
            let (zk_payment_status, erc20_payment_status) =
                payment_status(&cmd, &args.network, &config.account).await?;

            let token = match zk_payment_status.token.len() {
                0 => erc20_payment_status.token,
                _ => zk_payment_status.token,
            };

            let mut table = Table::new();
            let format = format::FormatBuilder::new().padding(1, 1).build();
            table.set_format(format);
            table.add_row(row![Style::new()
                .fg(Colour::Yellow)
                .underline()
                .paint("Wallet")]);
            table.add_empty_row();
            if let Some(account) = &config.account {
                table.add_row(row!["address", &account.address]);
            } else {
                table.add_row(row!["address", &id.node_id]);
            }
            table.add_row(row![
                "network",
                Style::new().fg(Colour::Purple).paint(&args.network)
            ]);
            let total_amount = &zk_payment_status.amount + &erc20_payment_status.amount;
            table.add_row(row![
                "amount (total)",
                format!("{} {}", total_amount, token)
            ]);
            table.add_row(row![
                "    (on-chain)",
                format!("{} {}", &erc20_payment_status.amount, token)
            ]);
            table.add_row(row![
                "     (zk-sync)",
                format!("{} {}", &zk_payment_status.amount, token)
            ]);
            table.add_empty_row();
            {
                let (pending, pending_cnt) = invoice_status.provider.total_pending();
                table.add_row(row![
                    "pending",
                    format!("{} {} ({})", pending, token, pending_cnt)
                ]);
            }
            let (unconfirmed, unconfirmed_cnt) = invoice_status.provider.unconfirmed();
            table.add_row(row![
                "issued",
                format!("{} {} ({})", unconfirmed, token, unconfirmed_cnt)
            ]);

            table
        };

        let activity = {
            let status = cmd.yagna()?.activity_status().await?;
            let mut table = Table::new();
            let format = format::FormatBuilder::new().padding(1, 1).build();
            table.set_format(format);
            table.add_row(row![Style::new()
                .fg(Colour::Yellow)
                .underline()
                .paint("Tasks")]);
            table.add_empty_row();
            table.add_row(row!["last 1h processed", status.last1h_processed()]);
            table.add_row(row!["last 1h in progress", status.in_progress()]);
            table.add_row(row!["total processed", status.total_processed()]);

            table
        };

        if size.0 > 120 {
            table.add_row(row![status, payments, activity]);
        } else {
            table.add_row(row![status]);
            table.add_row(row![payments]);
            table.add_row(row![activity]);
        }
    } else {
        table.add_row(row![status]);
    }
    table.printstd();
    if let Some(msg) = kvm_status.problem() {
        println!("\n VM problem: {}", msg);
    }
    Ok(0)
}
