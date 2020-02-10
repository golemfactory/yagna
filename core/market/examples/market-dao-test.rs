use chrono::Utc;

use ya_market::dao::{init, AgreementDao};
use ya_market::db::models::{AgreementState, NewAgreement};
use ya_market::Error;
use ya_persistence::executor::DbExecutor;

fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let db = DbExecutor::from_env()?;

    init(&db)?;

    let agreement_id = "zima";
    let new_agreement = NewAgreement {
        natural_id: agreement_id.to_string(),
        state_id: AgreementState::Proposal,
        demand_node_id: "".to_string(),
        demand_properties_json: "".to_string(),
        demand_constraints: "".to_string(),
        offer_node_id: "".to_string(),
        offer_properties_json: "".to_string(),
        offer_constraints: "".to_string(),
        valid_to: Utc::now().naive_utc(),
        approved_date: None,
        proposed_signature: "".to_string(),
        approved_signature: "".to_string(),
        committed_signature: None,
    };

    let conn = db.conn().map_err(Error::from)?;
    let agreement_dao = AgreementDao::new(&conn);
    agreement_dao.create(new_agreement)?;

    eprintln!("v={:?}", agreement_dao.get(agreement_id)?);

    Ok(())
}
