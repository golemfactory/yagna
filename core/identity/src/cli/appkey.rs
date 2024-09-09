use anyhow::Result;
use structopt::*;

use ya_core_model::appkey as model;
use ya_core_model::bus::GsbBindPoints;
use ya_core_model::identity as idm;
use ya_core_model::identity::IdentityInfo;
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::RpcEndpoint;

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
/// AppKey management
pub enum AppKeyCommand {
    Create {
        name: String,
        #[structopt(skip = model::DEFAULT_ROLE)]
        role: String,
        /// Select identity for this app-key.
        #[structopt(long)]
        id: Option<String>,
        /// Set cors policy for request made using this app-key.
        #[structopt(long)]
        allow_origins: Vec<String>,
    },
    Drop {
        name: String,
        #[structopt(long)]
        id: Option<String>,
    },
    List {
        #[structopt(long)]
        id: Option<String>,
        #[structopt(default_value = "1", short, long)]
        page: u32,
        #[structopt(default_value = "10", long)]
        per_page: u32,
    },
    Show {
        name: String,
    },
}

impl AppKeyCommand {
    async fn get_identity(gsb: GsbBindPoints, get_by: idm::Get) -> anyhow::Result<IdentityInfo> {
        gsb.local()
            .send(get_by)
            .await
            .map_err(anyhow::Error::msg)?
            .map_err(anyhow::Error::msg)?
            .ok_or_else(|| anyhow::Error::msg("Identity not found"))
    }

    pub async fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput> {
        let gsb = ctx.gsb.service(model::BUS_SERVICE_NAME);
        let ident_gsb = ctx.gsb.service(idm::BUS_SERVICE_NAME);

        match &self {
            AppKeyCommand::Create {
                name,
                role,
                id,
                allow_origins: allow_origin,
            } => {
                let identity = match id {
                    Some(id) => {
                        if id.starts_with("0x") {
                            id.parse()?
                        } else {
                            Self::get_identity(ident_gsb.clone(), idm::Get::ByAlias(id.into()))
                                .await?
                                .node_id
                        }
                    }
                    None => {
                        Self::get_identity(ident_gsb.clone(), idm::Get::ByDefault)
                            .await?
                            .node_id
                    }
                };
                let create = model::Create {
                    name: name.clone(),
                    role: role.clone(),
                    identity,
                    allow_origins: allow_origin.clone(),
                };
                let key = gsb.local().send(create).await??;
                Ok(CommandOutput::Object(serde_json::to_value(key)?))
            }
            AppKeyCommand::Drop { name, id } => {
                let remove = model::Remove {
                    name: name.clone(),
                    identity: id.clone(),
                };
                gsb.local()
                    .send(remove)
                    .await
                    .map_err(anyhow::Error::msg)?
                    .unwrap();
                Ok(CommandOutput::NoOutput)
            }
            AppKeyCommand::Show { name } => {
                let appkey = gsb
                    .local()
                    .send(model::GetByName { name: name.clone() })
                    .await
                    .map_err(anyhow::Error::msg)?
                    .unwrap();
                Ok(CommandOutput::Object(serde_json::to_value(appkey)?))
            }
            AppKeyCommand::List { id, page, per_page } => {
                let list = model::List {
                    identity: id.clone(),
                    page: *page,
                    per_page: *per_page,
                };
                let result: (Vec<model::AppKey>, u32) = gsb
                    .local()
                    .send(list)
                    .await
                    .map_err(anyhow::Error::msg)?
                    .unwrap();

                Ok(ResponseTable {
                    columns: vec![
                        "name".into(),
                        "key".into(),
                        "id".into(),
                        "role".into(),
                        "created".into(),
                    ],
                    values: result
                        .0
                        .into_iter()
                        .map(|app_key| {
                            serde_json::json! {[
                                app_key.name, app_key.key, app_key.identity,
                                app_key.role, app_key.created_date,
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
        }
    }
}
