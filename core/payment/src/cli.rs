// External crates
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use std::str::FromStr;
use std::time::UNIX_EPOCH;
use structopt::*;

// Workspace uses
use ya_core_model::{identity as id_api, payment::local as pay};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::accounts::{init_account, Account};
use crate::wallet;

/// Payment management.
#[derive(StructOpt, Debug)]
pub enum PaymentCli {
    /// List active payment accounts
    Accounts,

    /// Supply payment account with funds
    Fund {
        #[structopt(flatten)]
        account: pay::AccountCli,
    },

    /// Initialize payment account (i.e. make it ready for sending/receiving funds)
    Init {
        #[structopt(flatten)]
        account: pay::AccountCli,
        #[structopt(long, help = "Initialize account for sending")]
        sender: bool,
        #[structopt(long, help = "Initialize account for receiving")]
        receiver: bool,
    },

    /// Display account balance and a summary of sent/received payments
    Status {
        #[structopt(flatten)]
        account: pay::AccountCli,
        #[structopt(long, help = "Display account balance for the given time period")]
        last: Option<humantime::Duration>,
    },

    /// Enter layer 2 (deposit funds to layer 2 network)
    Enter {
        #[structopt(flatten)]
        account: pay::AccountCli,
        #[structopt(long)]
        amount: String,
    },

    /// Exit layer 2 (withdraw funds to Ethereum)
    Exit {
        #[structopt(flatten)]
        account: pay::AccountCli,
        #[structopt(
            long,
            help = "Optional address to exit to [default: <DEFAULT_IDENTITY>]"
        )]
        to_address: Option<String>,
        #[structopt(long, help = "Optional amount to exit [default: <ALL_FUNDS>]")]
        amount: Option<String>,
    },

    Transfer {
        #[structopt(flatten)]
        account: pay::AccountCli,
        #[structopt(long, help = "Recipient address")]
        to_address: String,
        #[structopt(long, help = "Amount in GLM for example 1.45")]
        amount: String,
        #[structopt(long, help = "Override gas price (in Gwei)", default_value = "auto")]
        gas_price: String,
        #[structopt(
            long,
            help = "Override maximum gas price (in Gwei)",
            default_value = "auto"
        )]
        max_gas_price: String,
        #[structopt(
            long,
            help = "Override gas limit (at least 48000 to account with GLM, 60000 to new account without GLM)",
            default_value = "auto"
        )]
        gas_limit: String,
    },
    Invoice {
        address: Option<String>,
        #[structopt(subcommand)]
        command: InvoiceCommand,
    },

    /// List registered drivers, networks, tokens and platforms
    Drivers,

    /// Clear all existing allocations
    ReleaseAllocations,
}

#[derive(StructOpt, Debug)]
pub enum InvoiceCommand {
    Status {
        #[structopt(long, help = "Display invoice status from the given period of time")]
        last: Option<humantime::Duration>,
    },
}

impl PaymentCli {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            PaymentCli::Fund { account } => CommandOutput::object(
                wallet::fund(
                    resolve_address(account.address()).await?,
                    account.driver(),
                    Some(account.network()),
                    None,
                )
                .await?,
            ),
            PaymentCli::Init {
                account,
                sender,
                receiver,
            } => {
                let account = Account {
                    driver: account.driver(),
                    address: resolve_address(account.address()).await?,
                    network: Some(account.network()),
                    token: None, // Use default -- we don't yet support other tokens than GLM
                    send: sender,
                    receive: receiver,
                };
                init_account(account).await?;
                Ok(CommandOutput::NoOutput)
            }
            PaymentCli::Status { account, last } => {
                let address = resolve_address(account.address()).await?;
                let timestamp = last
                    .map(|d| Utc::now() - chrono::Duration::seconds(d.as_secs() as i64))
                    .unwrap_or(DateTime::from(UNIX_EPOCH))
                    .timestamp();
                let status = bus::service(pay::BUS_ID)
                    .call(pay::GetStatus {
                        address: address.clone(),
                        driver: account.driver(),
                        network: Some(account.network()),
                        token: None,
                        after_timestamp: timestamp,
                    })
                    .await??;
                if ctx.json_output {
                    return CommandOutput::object(status);
                }

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
                            format!("driver: {}", status.driver),
                            format!("{} {}", status.amount, status.token),
                            format!("{} {}", status.reserved, status.token),
                            "accepted",
                            format!("{} {}", status.incoming.accepted.total_amount, status.token),
                            format!("{} {}", status.outgoing.accepted.total_amount, status.token),
                        ]},
                        serde_json::json! {[
                            format!("network: {}", status.network),
                            "",
                            "",
                            "confirmed",
                            format!("{} {}", status.incoming.confirmed.total_amount, status.token),
                            format!("{} {}", status.outgoing.confirmed.total_amount, status.token),
                        ]},
                        serde_json::json! {[
                            format!("token: {}", status.token),
                            "",
                            "",
                            "requested",
                            format!("{} {}", status.incoming.requested.total_amount, status.token),
                            format!("{} {}", status.outgoing.requested.total_amount, status.token),
                        ]},
                    ],
                }
                .with_header(format!("\nStatus for account: {}\n", address)))
            }
            PaymentCli::Accounts => {
                let accounts = bus::service(pay::BUS_ID)
                    .call(pay::GetAccounts {})
                    .await??;
                if ctx.json_output {
                    return CommandOutput::object(accounts);
                }

                Ok(ResponseTable {
                    columns: vec![
                        "address".to_owned(),
                        "driver".to_owned(),
                        "network".to_owned(),
                        "token".to_owned(),
                        "send".to_owned(),
                        "recv".to_owned(),
                    ],
                    values: accounts
                        .into_iter()
                        .map(|account| {
                            serde_json::json! {[
                                account.address,
                                account.driver,
                                account.network,
                                account.token,
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
            PaymentCli::Enter { account, amount } => CommandOutput::object(
                wallet::enter(
                    BigDecimal::from_str(&amount)?,
                    resolve_address(account.address()).await?,
                    account.driver(),
                    Some(account.network()),
                    None,
                )
                .await?,
            ),
            PaymentCli::Exit {
                account,
                to_address,
                amount,
            } => {
                let amount = match amount {
                    None => None,
                    Some(a) => Some(BigDecimal::from_str(&a)?),
                };
                CommandOutput::object(
                    wallet::exit(
                        resolve_address(account.address()).await?,
                        to_address,
                        amount,
                        account.driver(),
                        Some(account.network()),
                        None,
                    )
                    .await?,
                )
            }
            PaymentCli::Transfer {
                account,
                to_address,
                amount,
                gas_price,
                max_gas_price,
                gas_limit,
            } => {
                let address = resolve_address(account.address()).await?;
                let amount = BigDecimal::from_str(&amount)?;

                let gas_price = if gas_price.is_empty() || gas_price == "auto" {
                    None
                } else {
                    Some(BigDecimal::from_str(&gas_price)?)
                };
                let max_gas_price = if max_gas_price.is_empty() || max_gas_price == "auto" {
                    None
                } else {
                    Some(BigDecimal::from_str(&max_gas_price)?)
                };

                let gas_limit = if gas_limit.is_empty() || gas_limit == "auto" {
                    None
                } else {
                    Some(u32::from_str(&gas_limit)?)
                };

                CommandOutput::object(
                    wallet::transfer(
                        address,
                        to_address,
                        amount,
                        account.driver(),
                        Some(account.network()),
                        None,
                        gas_price,
                        max_gas_price,
                        gas_limit,
                    )
                    .await?,
                )
            }
            PaymentCli::Drivers => {
                let drivers = bus::service(pay::BUS_ID).call(pay::GetDrivers {}).await??;
                if ctx.json_output {
                    return CommandOutput::object(drivers);
                }
                Ok(ResponseTable { columns: vec![
                        "driver".to_owned(),
                        "network".to_owned(),
                        "default?".to_owned(),
                        "token".to_owned(),
                        "default?".to_owned(),
                        "platform".to_owned(),
                    ], values: drivers
                        .iter()
                        .flat_map(|(driver, dd)| {
                            dd.networks
                                .iter()
                                .flat_map(|(network, n)| {
                                    n.tokens
                                        .iter()
                                        .map(|(token, platform)|
                                            serde_json::json! {[
                                                driver,
                                                network,
                                                if &dd.default_network == network { "X" } else { "" },
                                                token,
                                                if &n.default_token == token { "X" } else { "" },
                                                platform,
                                            ]}
                                        )
                                        .collect::<Vec<serde_json::Value>>()
                                })
                                .collect::<Vec<serde_json::Value>>()
                        })
                        .collect()
                }.into())
            }
            PaymentCli::ReleaseAllocations => CommandOutput::object(
                bus::service(pay::BUS_ID)
                    .call(pay::ReleaseAllocations {})
                    .await??,
            ),
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
