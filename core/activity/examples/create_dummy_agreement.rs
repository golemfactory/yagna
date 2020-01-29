use diesel::prelude::*;
use serde_json::json;
use structopt::StructOpt;

use ya_persistence::executor::DbExecutor;

use ya_core_model::ethaddr::NodeId;
use ya_persistence::models::{AgreementState, NewAgreement};

fn gen_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(StructOpt)]
struct Args {
    requestor: NodeId,
    provider: NodeId,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let args = Args::from_args();

    let demand_props = json! {{
        "golem": {
            "node": {
                "id": {
                    "name": "dummy reqestor",
                },
                "geo": {
                    "country_code": "PL",
                }
            },
            "inf": {
                "activity": {
                    "timeout_secs": 30
                }
            }

        }
    }};

    let demand_constraints = r#"
        (&
            (golem.inf.mem.gib>=0.5)
            (golem.inf.storage.gib>=2)
            (golem.srv.comp.wasm.task_package=golemfactory/test01:v0)
        )
    "#;

    let data_dir = ya_service_api::default_data_dir()?;
    let db = DbExecutor::from_data_dir(&data_dir)?;

    db.apply_migration(ya_persistence::migrations::run_with_output)?;

    let natural_id = gen_id();

    let new_agreement = NewAgreement {
        natural_id,
        state_id: AgreementState::New,
        demand_node_id: args.requestor.to_string(),
        demand_properties_json: serde_json::to_string_pretty(&demand_props)?,
        demand_constraints_json: demand_constraints.to_string(),
        offer_node_id: args.provider.to_string(),
        offer_properties_json: "".to_string(),
        offer_constraints_json: "".to_string(),
        proposed_signature: "fake".to_string(),
        approved_signature: "fake".to_string(),
        committed_signature: None,
    };

    log::info!("inserting agreement: {:#?}", new_agreement);

    db.with_transaction(move |conn| {
        use ya_persistence::schema::agreement::dsl::agreement;
        diesel::insert_into(agreement)
            .values((&new_agreement,))
            .execute(conn)?;
        Ok::<_, anyhow::Error>(())
    })
    .await?;

    log::info!("done");
    Ok(())
}
