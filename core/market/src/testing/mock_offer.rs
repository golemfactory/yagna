use chrono::{Duration, NaiveDateTime, Utc};
use serde_json::Value;
use std::str::FromStr;

use ya_agreement_utils::agreement::flatten;
use ya_client::model::NodeId;

use crate::db::model::{Demand, Offer};
use crate::protocol::discovery::message::RetrieveOffers;
use crate::testing::mock_identity::generate_identity;
use crate::testing::SubscriptionId;

pub fn flatten_json(json: &Value) -> Value {
    let mapped = flatten(json.clone());
    Value::Object(mapped)
}

#[allow(unused)]
pub fn sample_retrieve_offers() -> RetrieveOffers {
    RetrieveOffers {
        offer_ids: vec![sample_offer().id],
    }
}

pub fn sample_offer() -> Offer {
    let creation_ts = Utc::now().naive_utc();
    let expiration_ts = creation_ts + Duration::hours(1);
    Offer::from_new(
        &client::sample_offer(),
        &generate_identity(""),
        creation_ts,
        expiration_ts,
    )
    .unwrap()
}

pub fn sample_offer_with_expiration(expiration_ts: NaiveDateTime) -> Offer {
    let creation_ts = Utc::now().naive_utc();
    Offer::from_new(
        &client::sample_offer(),
        &generate_identity(""),
        creation_ts,
        expiration_ts,
    )
    .unwrap()
}

pub fn generate_offer(id: &str, expiration_ts: NaiveDateTime) -> Offer {
    Offer {
        id: SubscriptionId::from_str(id).unwrap(),
        properties: "".to_string(),
        constraints: "".to_string(),
        node_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        creation_ts: Utc::now().naive_utc(),
        insertion_ts: None,
        expiration_ts,
    }
}

pub fn sample_demand() -> Demand {
    let creation_ts = Utc::now().naive_utc();
    let expiration_ts = creation_ts + Duration::hours(1);
    Demand::from_new(
        &client::sample_demand(),
        &generate_identity(""),
        creation_ts,
        expiration_ts,
    )
    .unwrap()
}

pub fn generate_demand(id: &str, expiration_ts: NaiveDateTime) -> Demand {
    Demand {
        id: SubscriptionId::from_str(id).unwrap(),
        properties: "".to_string(),
        constraints: "".to_string(),
        node_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        creation_ts: Utc::now().naive_utc(),
        insertion_ts: None,
        expiration_ts,
    }
}

pub mod client {
    use ya_agreement_utils::{constraints, ConstraintKey, Constraints};
    use ya_client::model::market::{NewDemand, NewOffer};

    #[allow(unused)]
    pub fn sample_offer() -> NewOffer {
        NewOffer::new(
            serde_json::json!({
                "golem": {
                    "node.id.name": "its-test-provider",
                    "node.debug.subnet": "blaa",
                    "com.pricing.model": "linear"
                },
            }),
            constraints![
                "golem.node.debug.subnet" == "blaa",
                "golem.srv.comp.expiration" > 0
            ]
            .to_string(),
        )
    }

    /// Use this Offer, if you want to negotiate with only subset of nodes.
    /// Set match_on parameter to unique string for this subset.
    #[allow(unused)]
    pub fn exclusive_offer(match_on: &str) -> NewOffer {
        NewOffer::new(
            serde_json::json!({
                "golem": {
                    "node.id.name": "its-test-provider",
                    "node.debug.subnet": "blaa",
                    "com.pricing.model": "linear",
                },
                "subnodes": match_on
            }),
            constraints![
                "golem.node.debug.subnet" == "blaa",
                "golem.srv.comp.expiration" > 0,
                "subnodes" == match_on
            ]
            .to_string(),
        )
    }

    #[allow(unused)]
    pub fn not_matching_offer() -> NewOffer {
        NewOffer::new(
            serde_json::json!({
                "golem": {
                    "node.id.name": "its-test-provider",
                    "node.debug.subnet": "blaa",
                    "com.pricing.model": "linear"
                },
            }),
            constraints![
                "golem.node.debug.subnet" == "blaa",
                "golem.srv.comp.expiration" > 0,
                "custom.dontmatch" == "true",
            ]
            .to_string(),
        )
    }

    #[allow(unused)]
    pub fn sample_demand() -> NewDemand {
        NewDemand::new(
            serde_json::json!({
                "golem": {
                    "node.id.name": "its-test-requestor",
                    "node.debug.subnet": "blaa",
                    "srv.comp.expiration": 3,
                    "srv.comp.task_package": "test-package",
                },
            }),
            constraints![
                "golem.node.debug.subnet" == "blaa",
                "golem.com.pricing.model" == "linear"
            ]
            .to_string(),
        )
    }

    /// Use this Demand, if you want to negotiate with only subset of nodes.
    /// Set match_on parameter to unique string for this subset.
    #[allow(unused)]
    pub fn exclusive_demand(match_on: &str) -> NewDemand {
        NewDemand::new(
            serde_json::json!({
                "golem": {
                    "node.id.name": "its-test-requestor",
                    "node.debug.subnet": "blaa",
                    "srv.comp.expiration": 3,
                    "srv.comp.task_package": "test-package",
                },
                "subnodes": match_on
            }),
            constraints![
                "golem.node.debug.subnet" == "blaa",
                "golem.com.pricing.model" == "linear",
                "subnodes" == match_on
            ]
            .to_string(),
        )
    }

    #[allow(unused)]
    pub fn not_matching_demand() -> NewDemand {
        NewDemand::new(
            serde_json::json!({
                "golem": {
                    "node.id.name": "its-test-requestor",
                    "node.debug.subnet": "blaa",
                    "srv.comp.expiration": 3,
                    "srv.comp.wasm.task_package": "test-package",
                },
            }),
            constraints![
                "golem.node.debug.subnet" == "blaa",
                "golem.com.pricing.model" == "linear",
                "custom.dontmatch" == "true",
            ]
            .to_string(),
        )
    }
}
