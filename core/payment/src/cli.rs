use crate::accounts::{init_account, Account};
use crate::DEFAULT_PAYMENT_DRIVER;
use chrono::Utc;
use structopt::*;
use ya_core_model::{identity as id_api, payment::local as pay};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

/// Payment management.
#[derive(StructOpt, Debug)]
pub enum PaymentCli {
    Init {
        address: Option<String>,
        #[structopt(long, short)]
        requestor: bool,
        #[structopt(long, short)]
        provider: bool,
        #[structopt(long, default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long)]
        network: Option<String>,
    },
    Status {
        #[structopt(long)]
        account: Option<String>,
        #[structopt(long, default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long)]
        network: Option<String>,
        #[structopt(long)]
        platform: Option<String>,
    },
    Accounts,
    Invoice {
        address: Option<String>,
        #[structopt(subcommand)]
        command: InvoiceCommand,
    },
}

#[derive(StructOpt, Debug)]
pub enum InvoiceCommand {
    Status {
        #[structopt(long)]
        last: Option<humantime::Duration>,
    },
}

impl PaymentCli {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            PaymentCli::Init {
                address,
                driver,
                network,
                requestor,
                provider,
            } => {
                let address = resolve_address(address).await?;
                let driver = driver.to_lowercase();
                let account = Account {
                    driver,
                    address,
                    network,
                    token: None, // Use default -- we don't yet support other tokens than GLM
                    send: requestor,
                    receive: provider,
                };
                init_account(account).await?;
                Ok(CommandOutput::NoOutput)
            }
            PaymentCli::Status {
                account,
                driver,
                network,
                platform,
            } => {
                let address = resolve_address(account).await?;
                let status = bus::service(pay::BUS_ID)
                    .call(pay::GetStatus {
                        address,
                        platform,
                        driver: Some(driver),
                        network,
                        token: None,
                    })
                    .await??;
                if ctx.json_output {
                    CommandOutput::object(status)
                } else {
                    Ok(ResponseTable {
                        columns: vec![
                            "platform".to_owned(),
                            "total amount".to_owned(),
                            "reserved".to_owned(),
                            "amount".to_owned(),
                            "incoming".to_owned(),
                            "outgoing".to_owned(),
                        ],
                        values: vec![
                            serde_json::json! {[
                            format!("driver: {}", status.platform.driver),
                            format!("{} {}", status.amount, status.platform.token),
                            format!("{} {}", status.reserved, status.platform.token),
                            "accepted",
                            status.incoming.accepted.total_amount,
                            status.outgoing.accepted.total_amount,
                            ]},
                            serde_json::json! {[
                            format!("network: {}", status.platform.network),
                            "",
                            "",
                            "confirmed",
                            status.incoming.confirmed.total_amount,
                            status.outgoing.confirmed.total_amount,
                            ]},
                            serde_json::json! {[
                            format!("token: {}", status.platform.token),
                            "",
                            "",
                            "requested",
                            status.incoming.requested.total_amount,
                            status.outgoing.requested.total_amount,
                            ]},
                        ],
                    }
                    .into())
                }
            }
            PaymentCli::Accounts => {
                let accounts = bus::service(pay::BUS_ID)
                    .call(pay::GetAccounts {})
                    .await??;
                Ok(ResponseTable {
                    columns: vec![
                        "platform".to_owned(),
                        "address".to_owned(),
                        "driver".to_owned(),
                        "send".to_owned(),
                        "recv".to_owned(),
                    ],
                    values: accounts
                        .into_iter()
                        .map(|account| {
                            serde_json::json! {[
                                account.platform,
                                account.address,
                                account.driver,
                                if account.send { "X" } else { "" },
                                if account.receive { "X" } else { "" }
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
            PaymentCli::Invoice {
                address,
                command: InvoiceCommand::Status { last },
            } => {
                let seconds = last.map(|d| d.as_secs() as i64).unwrap_or(3600);
                let address = resolve_address(address).await?;
                CommandOutput::object(
                    bus::service(pay::BUS_ID)
                        .call(pay::GetInvoiceStats::new(
                            address.parse()?,
                            Utc::now() + chrono::Duration::seconds(-seconds),
                        ))
                        .await??,
                )
            }
        }
    }
}

async fn resolve_address(address: Option<String>) -> anyhow::Result<String> {
    if let Some(id) = address {
        return Ok(id);
    }

    let id = bus::service(id_api::BUS_ID)
        .send(id_api::Get::ByDefault)
        .await??;

    if let Some(id) = id {
        return Ok(id.node_id.to_string());
    }

    anyhow::bail!("Default identity not found")
}
