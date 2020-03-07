use structopt::*;
use ya_core_model::{ethaddr::NodeId, identity as id_api, payment::local as pay};
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};

/// Payment management.
#[derive(StructOpt, Debug)]
pub enum PaymentCli {
    Init {
        identity: Option<NodeId>,
        #[structopt(long, short)]
        requestor: bool,
        #[structopt(long, short)]
        provider: bool,
    },
    Status {
        identity: Option<NodeId>,
    },
}

impl PaymentCli {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            PaymentCli::Init {
                identity,
                requestor,
                provider,
            } => {
                let identity = resolve_identity(identity).await?;
                bus::service(pay::BUS_ID)
                    .call(pay::Init {
                        identity,
                        requestor,
                        provider,
                    })
                    .await??;
                Ok(CommandOutput::NoOutput)
            }
            PaymentCli::Status { identity } => {
                let identity = resolve_identity(identity).await?;
                CommandOutput::object(
                    bus::service(pay::BUS_ID)
                        .call(pay::GetStatus::from(identity))
                        .await??,
                )
            }
        }
    }
}

async fn resolve_identity(identity: Option<NodeId>) -> anyhow::Result<NodeId> {
    if let Some(id) = identity {
        return Ok(id);
    }

    let id = bus::service(id_api::BUS_ID)
        .send(id_api::Get::ByDefault)
        .await??;

    if let Some(id) = id {
        return Ok(id.node_id);
    }

    anyhow::bail!("Default identity not found")
}
