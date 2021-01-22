// External crates
use bigdecimal::BigDecimal;
use chrono::Utc;
use std::str::FromStr;
use structopt::*;

// Workspace uses
use ya_core_model::{identity as id_api, payment::local as pay};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::accounts::{init_account, Account};
use crate::{wallet, DEFAULT_PAYMENT_DRIVER};

/// Payment management.
#[derive(StructOpt, Debug)]
pub enum PaymentCli {
    /// List active payment accounts
    Accounts,

    /// Supply payment account with funds
    Fund {
        #[structopt(long, help = "Wallet address [default: <DEFAULT_IDENTIDITY>]")]
        account: Option<String>,
        #[structopt(long, help = "Payment driver (zksync or erc20)", default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long, help = "Payment network (rinkeby or mainnet) [default: rinkeby]")]
        network: Option<String>,
    },

    /// Initialize payment account (i.e. make it ready for sending/receiving funds)
    Init {
        #[structopt(long, help = "Wallet address [default: <DEFAULT_IDENTIDITY>]")]
        account: Option<String>,
        #[structopt(long, help = "Initialize account for sending")]
        sender: bool,
        #[structopt(long, help = "Initialize account for receiving")]
        receiver: bool,
        #[structopt(long, help = "Payment driver (zksync or erc20)", default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long, help = "Payment network (rinkeby or mainnet) [default: rinkeby]")]
        network: Option<String>,
    },

    /// Display account balance and a summary of sent/received payments
    Status {
        #[structopt(long, help = "Wallet address [default: <DEFAULT_IDENTIDITY>]")]
        account: Option<String>,
        #[structopt(long, help = "Payment driver (zksync or erc20)", default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long, help = "Payment network (rinkeby or mainnet) [default: rinkeby]")]
        network: Option<String>,
    },

    // TODO: Uncomment when operation is supported by drivers
    // Enter {
    //     #[structopt(long)]
    //     account: Option<String>,
    //     #[structopt(long, default_value = DEFAULT_PAYMENT_DRIVER)]
    //     driver: String,
    //     #[structopt(long)]
    //     network: Option<String>,
    //     #[structopt(long)]
    //     amount: String,
    // },
    /// Exit layer 2 (withdraw funds to Ethereum)
    Exit {
        #[structopt(long, help = "Wallet address [default: <DEFAULT_IDENTIDITY>]")]
        account: Option<String>,
        #[structopt(long, help = "Payment driver (zksync or erc20)", default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long, help = "Payment network (rinkeby or mainnet) [default: rinkeby]")]
        network: Option<String>,
        #[structopt(
            long,
            help = "Optional address to exit to [default: <DEFAULT_IDENTIDITY>]"
        )]
        to_address: Option<String>,
        #[structopt(long, help = "Optional amount to exit [default: <ALL_FUNDS>]")]
        amount: Option<String>,
    },

    // TODO: Uncomment when operation is supported by drivers
    // Transfer {
    //     #[structopt(long)]
    //     account: Option<String>,
    //     #[structopt(long, default_value = DEFAULT_PAYMENT_DRIVER)]
    //     driver: String,
    //     #[structopt(long)]
    //     network: Option<String>,
    //     #[structopt(long)]
    //     to_address: String,
    //     #[structopt(long)]
    //     amount: String,
    // },
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
            PaymentCli::Fund {
                account,
                driver,
                network,
            } => {
                let address = resolve_address(account).await?;
                let driver = driver.to_lowercase();
                CommandOutput::object(wallet::fund(address, driver, network, None).await?)
            }
            PaymentCli::Init {
                account,
                driver,
                network,
                sender,
                receiver,
            } => {
                let address = resolve_address(account).await?;
                let driver = driver.to_lowercase();
                let account = Account {
                    driver,
                    address,
                    network,
                    token: None, // Use default -- we don't yet support other tokens than GLM
                    send: sender,
                    receive: receiver,
                };
                init_account(account).await?;
                Ok(CommandOutput::NoOutput)
            }
            PaymentCli::Status {
                account,
                driver,
                network,
            } => {
                let address = resolve_address(account).await?;
                let status = bus::service(pay::BUS_ID)
                    .call(pay::GetStatus {
                        address,
                        driver,
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
                    .into())
                }
            }
            PaymentCli::Accounts => {
                let accounts = bus::service(pay::BUS_ID)
                    .call(pay::GetAccounts {})
                    .await??;
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
            // TODO: Uncomment when operation is supported by drivers
            // PaymentCli::Enter {
            //     account,
            //     driver,
            //     network,
            //     amount
            // } => {
            //     let address = resolve_address(account).await?;
            //     let amount = BigDecimal::from_str(&amount)?;
            //     CommandOutput::object(wallet::enter(amount, address, driver, network, token).await?)
            // }
            PaymentCli::Exit {
                account,
                driver,
                network,
                to_address,
                amount,
            } => {
                let address = resolve_address(account).await?;
                let amount = match amount {
                    None => None,
                    Some(a) => Some(BigDecimal::from_str(&a)?),
                };
                CommandOutput::object(
                    wallet::exit(address, to_address, amount, driver, network, None).await?,
                )
            } // TODO: Uncomment when operation is supported by drivers
              // PaymentCli::Transfer {
              //     account,
              //     driver,
              //     network,
              //     to_address,
              //     amount
              // } => {
              //     let address = resolve_address(account).await?;
              //     let amount = BigDecimal::from_str(&amount)?;
              //     CommandOutput::object(wallet::transfer(address, to_address, amount, driver, network, token).await?)
              // }
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
