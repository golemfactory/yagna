use crate::accounts::{init_account, Account};
use crate::{DEFAULT_PAYMENT_DRIVER, DEFAULT_PAYMENT_PLATFORM};
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
    },
    Status {
        address: Option<String>,
        #[structopt(long, short)]
        platform: Option<String>,
    },
    Accounts,
}

impl PaymentCli {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            PaymentCli::Init {
                address,
                driver,
                requestor,
                provider,
            } => {
                let address = resolve_address(address).await?;
                let driver = driver.to_lowercase();
                let account = Account {
                    driver,
                    address,
                    send: requestor,
                    receive: provider,
                };
                init_account(account).await?;
                Ok(CommandOutput::NoOutput)
            }
            PaymentCli::Status { address, platform } => {
                let address = resolve_address(address).await?;
                let platform = platform
                    .unwrap_or(DEFAULT_PAYMENT_PLATFORM.to_owned())
                    .to_uppercase();
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
