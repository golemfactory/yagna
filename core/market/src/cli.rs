use chrono::{DateTime, Utc};
use structopt::StructOpt;
use ya_client::model::market::{agreement::State, Role};
use ya_core_model::market::{GetAgreement, ListAgreements};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

/// Market management
#[derive(StructOpt, Debug)]
pub enum Command {
    Agreements(AgreementsCommand),
}

impl Command {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            Command::Agreements(agreements_cmd) => agreements_cmd.run_command(ctx).await,
        }
    }
}

#[derive(StructOpt, Debug)]
pub enum AgreementsCommand {
    List {
        #[structopt(long, help = "Only show agreements with this state")]
        state: Option<State>,
        #[structopt(long, help = "Only show agreements after this date, rfc3339")]
        before: Option<DateTime<Utc>>,
        #[structopt(long, help = "Only show agreements before this date, rfc3339")]
        after: Option<DateTime<Utc>>,
        #[structopt(long, help = "Only show agreements with this app session id")]
        app_session_id: Option<String>,
    },
    Get {
        #[structopt(long, help = "Agreement ID, may be obtained via list-agreements")]
        id: String,
        #[structopt(long, help = "Your role in the agreement (Provider | Requestor)")]
        role: Role,
    },
}

impl AgreementsCommand {
    pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            AgreementsCommand::List {
                state,
                before,
                after,
                app_session_id,
            } => {
                let request = ListAgreements {
                    state: state,
                    before_date: before,
                    after_date: after,
                    app_session_id,
                };

                let agreements = bus::service(ya_core_model::market::BUS_ID)
                    .send(request)
                    .await??;

                let mut agreements_json = Vec::new();
                for agreement in agreements {
                    agreements_json.push(serde_json::to_value([
                        agreement.id,
                        agreement.role.to_string(),
                        agreement.timestamp.to_rfc3339(),
                        agreement
                            .approved_date
                            .map(|ts| ts.to_rfc3339())
                            .unwrap_or_else(|| "N/A".to_owned()),
                    ])?);
                }

                Ok(ResponseTable {
                    columns: vec![
                        "id".to_owned(),
                        "role".to_owned(),
                        "created".to_owned(),
                        "approved".to_owned(),
                    ],
                    values: agreements_json,
                }
                .with_header("\nMatching agreements:\n".to_owned()))
            }
            AgreementsCommand::Get { id, role } => {
                let request = GetAgreement {
                    agreement_id: id,
                    role,
                };

                let agreement = bus::service(ya_core_model::market::BUS_ID)
                    .send(request)
                    .await??;

                CommandOutput::object(agreement)
            }
        }
    }
}
