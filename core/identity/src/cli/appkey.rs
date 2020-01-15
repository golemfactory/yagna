use anyhow::Result;
use structopt::*;

use ya_core_model::appkey as model;
use ya_core_model::identity as idm;
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum AppKeyCommand {
    Create {
        name: String,
        #[structopt(default_value = model::DEFAULT_ROLE, short, long)]
        role: String,
        #[structopt(default_value = model::DEFAULT_IDENTITY, long)]
        id: String,
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
}

impl AppKeyCommand {
    pub async fn run_command(&self, _ctx: &CliCtx) -> Result<CommandOutput> {
        match &self {
            AppKeyCommand::Create { name, role, id } => {
                let identity = if id.starts_with("0x") {
                    id.parse()?
                } else {
                    let key = bus::service(idm::BUS_ID)
                        .send(idm::Get::ByAlias(id.into()))
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?
                        .map_err(|e| anyhow::Error::msg(e))?;
                    key.unwrap().node_id
                };

                let create = model::Create {
                    name: name.clone(),
                    role: role.clone(),
                    identity,
                };
                let key = bus::service(model::APP_KEY_SERVICE_ID)
                    .send(create)
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?
                    .unwrap();
                Ok(CommandOutput::Object(serde_json::to_value(key)?))
            }
            AppKeyCommand::Drop { name, id } => {
                let remove = model::Remove {
                    name: name.clone(),
                    identity: id.clone(),
                };
                let _ = bus::service(model::APP_KEY_SERVICE_ID)
                    .send(remove)
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?
                    .unwrap();
                Ok(CommandOutput::NoOutput)
            }
            AppKeyCommand::List { id, page, per_page } => {
                let list = model::List {
                    identity: id.clone(),
                    page: page.clone(),
                    per_page: per_page.clone(),
                };
                let result: (Vec<model::AppKey>, u32) = bus::service(model::APP_KEY_SERVICE_ID)
                    .send(list)
                    .await
                    .map_err(|e| anyhow::Error::msg(e))?
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
                                app_key.role, app_key.created_date
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
        }
    }
}
