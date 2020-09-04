use chrono::{Duration, NaiveDateTime, Utc};
use std::str::FromStr;

use crate::db::model::{Agreement, AgreementState, OwnerType, ProposalId, SubscriptionId};
use ya_client::model::NodeId;

pub fn generate_agreement(unifier: i64, valid_to: NaiveDateTime) -> Agreement {
    let id = ProposalId::generate_id(
        &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53", ).unwrap(),
        &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53", ).unwrap(),
        // Add parametrized integer - unifier to ensure unique ids
        &(Utc::now() + Duration::days(unifier)).naive_utc(),
        OwnerType::Requestor,
    );
    Agreement {
        id: id.clone(),
        offer_properties: "".to_string(),
        offer_constraints: "".to_string(),
        demand_properties: "".to_string(),
        demand_constraints: "".to_string(),
        offer_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        demand_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        offer_proposal_id: id.clone().translate(OwnerType::Provider),
        demand_proposal_id: id,
        provider_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        requestor_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        creation_ts: Utc::now().naive_utc(),
        valid_to,
        approved_date: None,
        state: AgreementState::Proposal,
        proposed_signature: None,
        approved_signature: None,
        committed_signature: None,
    }
}
