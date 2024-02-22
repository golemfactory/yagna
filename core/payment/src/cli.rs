// External crates
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::{json, to_value};
use std::str::FromStr;
use std::time::UNIX_EPOCH;
use structopt::*;
use ya_client_model::payment::DriverStatusProperty;
use ya_core_model::payment::local::{DriverName, NetworkName};

// Workspace uses
use ya_core_model::{identity as id_api, payment::local as pay};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::accounts::{init_account, Account};
use crate::wallet;

/// Payment driver management.
#[derive(StructOpt, Debug)]
pub enum DriverSubcommand {
    /// List registered drivers, networks, tokens and platforms
    List,

    /// Display status of the payment driver
    Status {
        #[structopt(flatten)]
        account: pay::AccountCli,
    },

    /// Display Web3 RPC endpoints and their status for the driver
    Rpc {
        #[structopt(flatten)]
        account: pay::AccountCli,

        #[structopt(long, help = "Show info for all networks")]
        all: bool,
    },
}

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
        #[structopt(long, help = "Show exact balances instead of rounding")]
        precise: bool,
    },

    Driver {
        #[structopt(subcommand)]
        command: DriverSubcommand,
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

        #[structopt(
            long,
            help = "Use gasless forwarder, no gas on account is required",
            conflicts_with_all(&["gas-limit", "max-gas-price", "gas-price"])
        )]
        gasless: bool,
    },
    Invoice {
        address: Option<String>,
        #[structopt(subcommand)]
        command: InvoiceCommand,
    },

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
            PaymentCli::Fund { account } => {
                if !account.network.is_fundable() {
                    log::error!(
                        "Network {} does not support automatic funding. Consider using one of the following: {:?}",
                        account.network,
                        NetworkName::all_fundable(),
                    );

                    return CommandOutput::object("Failed");
                }

                let address = resolve_address(account.address()).await?;

                init_account(Account {
                    driver: account.driver(),
                    address: address.clone(),
                    network: Some(account.network()),
                    token: None, // Use default -- we don't yet support other tokens than GLM
                    send: true,
                    receive: false,
                })
                .await?;
                let warn_message = r#"Sending fund request to yagna service, observe yagna log for details.
Typically operation should take less than 1 minute.
  It may get stuck due to
    1. problems with web3 RPC connection
    2. unusual high gas price
    3. problems with faucet
  If stuck for too long you can stop safely with Ctrl-C and try again later
"#;
                log::warn!("{}", warn_message);

                CommandOutput::object(
                    wallet::fund(address, account.driver(), Some(account.network()), None).await?,
                )
            }
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

            PaymentCli::Status {
                account,
                last,
                precise,
            } => {
                let address = resolve_address(account.address()).await?;
                let timestamp = last
                    .map(|d| Utc::now() - chrono::Duration::seconds(d.as_secs() as i64))
                    .unwrap_or_else(|| DateTime::from(UNIX_EPOCH))
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

                let gas_info = match status.gas.as_ref() {
                    Some(details) => {
                        if precise {
                            format!("{} {}", details.balance, details.currency_short_name)
                        } else {
                            format!("{:.4} {}", details.balance, details.currency_short_name)
                        }
                    }
                    None => "N/A".to_string(),
                };

                let token_info = if precise {
                    format!("{} {}", status.amount, status.token)
                } else {
                    format!("{:.4} {}", status.amount, status.token)
                };

                let driver_status_props = bus::service(pay::BUS_ID)
                    .call(pay::PaymentDriverStatus {
                        driver: Some(account.driver()),
                        network: Some(account.network()),
                    })
                    .await??;

                let mut header = format!("\nStatus for account: {}\n", address);
                if driver_status_props.is_empty() {
                    header.push_str("Payment Driver status: OK\n");
                } else {
                    header.push_str("\nPayment Driver status:\n");
                    for prop in driver_status_props {
                        use DriverStatusProperty::*;

                        let network = match &prop {
                            CantSign { network, .. }
                            | InsufficientGas { network, .. }
                            | InsufficientToken { network, .. }
                            | RpcError { network, .. }
                            | TxStuck { network, .. } => network.clone(),
                            InvalidChainId { .. } => "unknown network".to_string(),
                        };

                        header.push_str(&format!("Network:{network} - "));

                        match prop {
                            CantSign { address, .. } => {
                                header.push_str(&format!("Outstanding payments for address {address} cannot be signed. Is the relevant identity locked?\n"));
                            }
                            InsufficientGas { needed_gas_est, .. } => {
                                header.push_str(&format!("Not enough gas to send any more transactions. To send out all scheduled transactions additionally {}{} is needed.\n", needed_gas_est,
                                                         status.clone().gas.map(|g|g.currency_short_name.clone()).unwrap_or("ETH".to_string())
                                ));
                            }
                            InsufficientToken {
                                needed_token_est, ..
                            } => {
                                header.push_str(&format!("Not enough token to send any more transactions. To send out all scheduled transactions approximately {}{} is needed.\n", needed_token_est, status.token));
                            }
                            InvalidChainId { chain_id, .. } => {
                                header.push_str(&format!("Scheduled transactions on chain with id = {chain_id}, but no such chain is configured.\n"));
                            }
                            RpcError { network, .. } => {
                                header.push_str(&format!("RPC endpoints configured for {network} are unreliable. Consider changing them.\n"));
                            }
                            TxStuck { .. } => {
                                header.push_str("Sent transactions are stuck. Consider increasing max fee per gas.\n");
                            }
                        }
                    }
                }

                Ok(ResponseTable {
                    columns: vec![
                        "platform".to_owned(),
                        "total amount".to_owned(),
                        "reserved".to_owned(),
                        "amount".to_owned(),
                        "incoming".to_owned(),
                        "outgoing".to_owned(),
                        "gas".to_owned(),
                    ],
                    values: vec![
                        serde_json::json! {[
                            format!("driver: {}", status.driver),
                            token_info,
                            format!("{} {}", status.reserved, status.token),
                            "accepted",
                            format!("{} {}", status.incoming.accepted.total_amount, status.token),
                            format!("{} {}", status.outgoing.accepted.total_amount, status.token),
                            gas_info,
                        ]},
                        serde_json::json! {[
                            format!("network: {}", status.network),
                            "",
                            "",
                            "confirmed",
                            format!("{} {}", status.incoming.confirmed.total_amount, status.token),
                            format!("{} {}", status.outgoing.confirmed.total_amount, status.token),
                            ""
                        ]},
                        serde_json::json! {[
                            format!("token: {}", status.token),
                            "",
                            "",
                            "requested",
                            format!("{} {}", status.incoming.requested.total_amount, status.token),
                            format!("{} {}", status.outgoing.requested.total_amount, status.token),
                            ""
                        ]},
                    ],
                }
                .with_header(header))
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
                gasless,
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
                        gasless,
                    )
                    .await?,
                )
            }
            PaymentCli::Driver { command } => {
                match command {
                    DriverSubcommand::Rpc { account, all } => {
                        let address = resolve_address(account.address()).await?;
                        let driver = DriverName::from_str(&account.driver()).map_err(|e| {
                            anyhow::anyhow!(
                                "Invalid driver name: {}. Error: {}",
                                account.driver(),
                                e
                            )
                        })?;

                        let network = account.network();
                        //let network = network.to_string();
                        if driver != DriverName::Erc20 {
                            log::error!("Only ERC20 driver is supported for now");
                            return Err(anyhow::anyhow!(
                                "Only ERC20 driver is supported for this command"
                            ));
                        }

                        let status = bus::service(pay::BUS_ID)
                            .call(pay::GetRpcEndpoints {
                                address,
                                driver: driver.to_string(),
                                network: if all { None } else { Some(network.to_string()) },
                            })
                            .await??;

                        if ctx.json_output {
                            Ok(CommandOutput::Object(status.response))
                        } else {
                            Ok(CommandOutput::Table {
                                columns: vec!["network".to_owned(), "url".to_owned()],
                                values: vec![json!([{"network": "network"}, {"url": "url"}])],
                                summary: vec![
                                    json!([{"network": "network summary"}, {"url": "url summary"}]),
                                ],
                                header: Some(format!(
                                    "Endpoints for driver {} and network {}",
                                    driver, network
                                )),
                            })
                        }
                    }

                    DriverSubcommand::Status { account } => {
                        let driver_status_props = bus::service(pay::BUS_ID)
                            .call(pay::PaymentDriverStatus {
                                driver: Some(account.driver()),
                                network: Some(account.network()),
                            })
                            .await??;

                        if ctx.json_output {
                            return CommandOutput::object(driver_status_props);
                        }

                        let ok_msg = if driver_status_props.is_empty() {
                            "\nDriver Status: Ok"
                        } else {
                            ""
                        };

                        Ok(ResponseTable {
                            columns: vec!["issues".to_owned()],
                            values: driver_status_props
                                .into_iter()
                                .map(|prop| match prop {
                                    DriverStatusProperty::CantSign { address, .. } => {
                                        format!("Can't sign {address}")
                                    }
                                    DriverStatusProperty::InsufficientGas {
                                        needed_gas_est,
                                        ..
                                    } => {
                                        format!("Insufficient gas (need est. {needed_gas_est})")
                                    }
                                    DriverStatusProperty::InsufficientToken {
                                        needed_token_est,
                                        ..
                                    } => {
                                        format!("Insufficient token (need est. {needed_token_est})")
                                    }
                                    DriverStatusProperty::InvalidChainId { chain_id, .. } => {
                                        format!("Invalid Chain-Id ({chain_id})")
                                    }
                                    DriverStatusProperty::RpcError { network, .. } => {
                                        format!("Unreliable {network} RPC endpoints")
                                    }
                                    DriverStatusProperty::TxStuck { network, .. } => {
                                        format!("Tx stuck on {network}")
                                    }
                                })
                                .map(|s| to_value(vec![to_value(s).unwrap()]).unwrap())
                                .collect::<Vec<_>>(),
                        }
                        .with_header(format!(
                            "Status of the {} payment driver{}",
                            account.driver(),
                            ok_msg
                        )))
                    }
                    DriverSubcommand::List => {
                        let drivers = bus::service(pay::BUS_ID).call(pay::GetDrivers {}).await??;
                        if ctx.json_output {
                            return CommandOutput::object(drivers);
                        }
                        Ok(ResponseTable {
                                columns: vec![
                                    "driver".to_owned(),
                                    "network".to_owned(),
                                    "default?".to_owned(),
                                    "token".to_owned(),
                                    "platform".to_owned(),
                                ],
                                values: drivers
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
                                                platform,
                                            ]}
                                                    )
                                                    .collect::<Vec<serde_json::Value>>()
                                            })
                                            .collect::<Vec<serde_json::Value>>()
                                    })
                                    .collect(),
                            }.into())
                    }
                }
            }
            PaymentCli::ReleaseAllocations => {
                let _ = bus::service(pay::BUS_ID)
                    .call(pay::ReleaseAllocations {})
                    .await;
                Ok(CommandOutput::NoOutput)
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
