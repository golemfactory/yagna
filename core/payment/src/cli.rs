use crate::DEFAULT_PAYMENT_PLATFORM;
use structopt::*;
use ya_core_model::{driver, identity as id_api, payment::local as pay};
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};
use ya_core_model::driver::driver_bus_id;

/// Payment management.
#[derive(StructOpt, Debug)]
pub enum PaymentCli {
    Init {
        driver: String,
        address: Option<String>,
        #[structopt(long, short)]
        requestor: bool,
        #[structopt(long, short)]
        provider: bool,
    },
    Status {
        address: Option<String>,
        #[structopt(long, short)]
        platform: Option<String>,
    },
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
                let mut mode = driver::AccountMode::NONE;
                if requestor {
                    mode |= driver::AccountMode::SEND;
                }
                if provider {
                    mode |= driver::AccountMode::RECV;
                }
                bus::service(driver_bus_id(driver))
                    .call(driver::Init::new(address, mode))
                    .await??;
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
