use crate::command::YaCommand;
use crate::utils::is_yagna_running;
use ansi_term::{Colour, Style};
use anyhow::Result;
use futures::prelude::*;
use prettytable::{cell, format, row, Table};

pub async fn run() -> Result</*exit code*/ i32> {
    let cmd = YaCommand::new()?;
    let _top = ['\u{256d}', '\u{2500}', '\u{256e}'];
    let _mid = ['\u{2502}'];

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
        table.add_empty_row();
        table.add_row(row!["Node Name", &config.node_name.unwrap_or_default()]);
        table.add_row(row!["Subnet", &config.subnet.unwrap_or_default()]);
        table
    };
    let mut table = Table::new();
    table.set_format(*format::consts::FORMAT_BOX_CHARS);

    if is_running {
        let payments = {
            let (id, payment_status) =
                future::try_join(cmd.yagna()?.default_id(), cmd.yagna()?.payment_status()).await?;

            let mut table = Table::new();
            let format = format::FormatBuilder::new().padding(1, 1).build();
            table.set_format(format);
            table.add_row(row![Style::new()
                .fg(Colour::Yellow)
                .underline()
                .paint("Wallet")]);
            table.add_empty_row();
            table.add_row(row!["address", &id.node_id]);
            table.add_row(row!["amount", format!("{} NGNT", &payment_status.amount)]);
            table.add_row(row![
                "pending",
                format!("{} NGNT", &payment_status.incoming.total_pending())
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

        table.add_row(row![status, payments, activity]);
    } else {
        table.add_row(row![status]);
    }
    table.printstd();

    Ok(0)
}
