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
use crate::{wallet, DEFAULT_PAYMENT_DRIVER, DEFAULT_PAYMENT_PLATFORM};

/// Payment management.
#[derive(StructOpt, Debug)]
pub enum PaymentCli {
    Accounts,
    Enter {
        amount: String,
        #[structopt(long, default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long, short)]
        network: Option<String>,
        #[structopt(long, short)]
        token: Option<String>,
    },
    Exit {
        address: Option<String>,
        #[structopt(help = "Optional address to exit to. [default: <DEFAULT_IDENTIDITY>]")]
        to: Option<String>,
        #[structopt(long, short, help = "Optional amount to exit. [default: <ALL_FUNDS>]")]
        amount: Option<String>,
        #[structopt(long, default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long, short)]
        network: Option<String>,
        #[structopt(long, short)]
        token: Option<String>,
    },
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
    Invoice {
        address: Option<String>,
        #[structopt(subcommand)]
        command: InvoiceCommand,
    },
    Transfer {
        to: String,
        #[structopt(long, short)]
        amount: String,
        #[structopt(long, default_value = DEFAULT_PAYMENT_DRIVER)]
        driver: String,
        #[structopt(long, short)]
        network: Option<String>,
        #[structopt(long, short)]
        token: Option<String>,
    },
    Status {
        address: Option<String>,
        #[structopt(long, short)]
        platform: Option<String>,
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
            PaymentCli::Status { address, platform } => {
                let address = resolve_address(address).await?;
                let platform = platform.unwrap_or(DEFAULT_PAYMENT_PLATFORM.to_owned());
                CommandOutput::object(
                    bus::service(pay::BUS_ID)
                        .call(pay::GetStatus { address, platform })
                        .await??,
                )
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
            PaymentCli::Enter {
                amount,
                driver,
                network,
                token,
            } => {
                let amount = BigDecimal::from_str(&amount)?;
                CommandOutput::object(wallet::enter(amount, driver, network, token).await?)
            }
            PaymentCli::Exit {
                address,
                to,
                amount,
                driver,
                network,
                token,
            } => {
                let address = resolve_address(address).await?;
                let amount = match amount {
                    None => None,
                    Some(a) => Some(BigDecimal::from_str(&a)?),
                };
                CommandOutput::object(
                    wallet::exit(address, to, amount, driver, network, token).await?,
                )
            }
            PaymentCli::Transfer {
                to,
                amount,
                driver,
                network,
                token,
            } => {
                let amount = BigDecimal::from_str(&amount)?;
                CommandOutput::object(wallet::transfer(to, amount, driver, network, token).await?)
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
