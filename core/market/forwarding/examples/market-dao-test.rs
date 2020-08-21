use chrono::Utc;

use ya_market_forwarding::dao::{init, AgreementDao};
use ya_market_forwarding::db::models::{AgreementState, NewAgreement};
use ya_persistence::executor::DbExecutor;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let db = DbExecutor::from_env()?;

    init(&db)?;

    let agreement_id = "zima";
    let new_agreement = NewAgreement {
        natural_id: agreement_id.to_string(),
        state_id: AgreementState::Proposal,
        demand_node_id: "demand_node_id".to_string(),
        demand_properties_json: "demand_properties_json".to_string(),
        demand_constraints: "demand_constraints".to_string(),
        offer_node_id: "offer_node_id".to_string(),
        offer_properties_json: "offer_properties_json".to_string(),
        offer_constraints: "offer_constraints".to_string(),
        valid_to: Utc::now().naive_utc(),
        approved_date: None,
        proposed_signature: "proposed_signature".to_string(),
        approved_signature: "approved_signature".to_string(),
        committed_signature: None,
    };

    let agreement_dao = db.as_dao::<AgreementDao>();
    agreement_dao.create(new_agreement).await?;

    eprintln!("v={:?}", agreement_dao.get(agreement_id.into()).await?);

    Ok(())
}
