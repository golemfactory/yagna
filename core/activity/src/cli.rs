use structopt::StructOpt;
use ya_core_model::activity::local as acm;
use ya_core_model::identity as idm;
use ya_core_model::identity::IdentityInfo;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};

/// Activity management.
#[derive(StructOpt, Debug)]
pub enum ActivityCli {
    Status {
        #[structopt(long)]
        id: Option<String>,
    },
}

impl ActivityCli {
    async fn get_identity(get_by: idm::Get) -> anyhow::Result<IdentityInfo> {
        Ok(bus::service(idm::BUS_ID).send(get_by).await??)
    }

    pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            ActivityCli::Status { id } => {
                let identity = match id {
                    Some(id) => {
                        if id.starts_with("0x") {
                            id.parse()?
                        } else {
                            Self::get_identity(idm::Get::ByAlias(id.into()))
                                .await?
                                .node_id
                        }
                    }
                    None => Self::get_identity(idm::Get::ByDefault).await?.node_id,
                };
                let result = bus::service(acm::BUS_ID)
                    .send(acm::Stats { identity })
                    .await??;

                CommandOutput::object(result)
            }
        }
    }
}
