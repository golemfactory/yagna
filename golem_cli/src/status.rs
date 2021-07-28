use ansi_term::{Colour, Style};
use anyhow::{anyhow, Result};
use futures::prelude::*;
use prettytable::{cell, format, row, Table};
use strum::VariantNames;

use ya_core_model::payment::local::{NetworkName, StatusResult};
use ya_core_model::NodeId;

use crate::appkey;
use crate::command::{PaymentSummary, YaCommand, ERC20_DRIVER, ZKSYNC_DRIVER};
use crate::platform::Status as KvmStatus;
use crate::utils::{is_yagna_running, payment_account};

async fn payment_status(
    cmd: &YaCommand,
    network: &NetworkName,
    account: &Option<NodeId>,
) -> anyhow::Result<(StatusResult, StatusResult)> {
    let address = payment_account(&cmd, account).await?;

    let (status_zk, status_erc20) = future::join(
        cmd.yagna()?
            .payment_status(&address, network, &ZKSYNC_DRIVER),
        cmd.yagna()?
            .payment_status(&address, network, &ERC20_DRIVER),
    )
    .await;

    match (status_zk, status_erc20) {
        (Ok(zk), Ok(eth)) => Ok((zk, eth)),
        (Ok(zk), Err(e)) => {
            log::warn!("yagna payment status for ERC-20 {} failed: {}", network, e);
            Ok((zk, StatusResult::default()))
        }
        (Err(e), Ok(erc20)) => {
            log::warn!("yagna payment status for zkSync {} failed: {}", network, e);
            Ok((StatusResult::default(), erc20))
        }
        (Err(e), _) => Err(e),
    }
}

pub async fn run() -> Result</*exit code*/ i32> {
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
            if let Some(pending) = cmd.yagna()?.version().await?.pending {
                let ver = format!("{} released!", pending.version);
                table.add_row(row![
                    "New Version",
                    Style::new().fg(Colour::Fixed(220)).paint(ver)
                ]);
            }
        } else {
            table.add_row(row![
                "Service",
                Style::new().fg(Colour::Red).paint("is not running")
            ]);
        }
        table.add_row(row!["Version", ya_compile_time_utils::semver_str!()]);
        table.add_row(row!["Commit", ya_compile_time_utils::git_rev()]);
        table.add_row(row!["Date", ya_compile_time_utils::build_date()]);
        table.add_row(row![
            "Build",
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
        let (_offers_cnt, network) = get_payment_network().await?;

        let payments = {
            let (id, invoice_status) =
                future::try_join(cmd.yagna()?.default_id(), cmd.yagna()?.invoice_status()).await?;
            let (zk_payment_status, erc20_payment_status) =
                payment_status(&cmd, &network, &config.account).await?;

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
            let account = config.account.map(|a| a.to_string()).unwrap_or(id.node_id);
            table.add_row(row![H2->Style::new().fg(Colour::Fixed(63)).paint(&account)]);
            table.add_empty_row();

            let net_color = match network {
                NetworkName::Mainnet => Colour::Purple,
                NetworkName::Rinkeby => Colour::Cyan,
                _ => Colour::Red,
            };

            table.add_row(row![
                "network",
                Style::new().fg(net_color).paint(network.to_string())
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
            // table.add_row(row![Style::new()
            //     .fg(Colour::Yellow)
            //     .underline()
            //     .paint("Offers")]);
            // table.add_empty_row();
            // table.add_row(row!["Subscribed", offers_cnt]);
            // table.add_empty_row();
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

async fn get_payment_network() -> Result<(usize, NetworkName)> {
    // Dirty hack: we determine currently used payment network by checking latest offer properties
    let app_key = appkey::get_app_key().await?;
    let mkt_api: ya_client::market::MarketProviderApi =
        ya_client::web::WebClient::with_token(&app_key).interface()?;
    let offers = mkt_api.get_offers().await?;

    let latest_offer = offers.iter().max_by_key(|o| o.timestamp).ok_or(anyhow!(
        "Provider is not functioning properly. No offers Subscribed."
    ))?;
    let mut network = None;
    for net in NetworkName::VARIANTS {
        let net_to_check = net.parse()?;
        let platform_property = &format!(
            "golem.com.payment.platform.{}.address",
            ZKSYNC_DRIVER.platform(&net_to_check)?.platform
        );
        if latest_offer.properties.get(platform_property).is_some() {
            network = Some(net_to_check)
        };
    }

    let network = network.ok_or(anyhow!(
        "Unable to determine payment network used by the Yagna Provider."
    ))?;
    Ok((offers.len(), network))
}
