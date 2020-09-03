use chrono::{Duration, NaiveDateTime, Utc};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::str::FromStr;
use std::string::ToString;

use ya_client::model::NodeId;
use ya_service_api_web::middleware::Identity;

use crate::db::model::{Demand, Offer, SubscriptionId};
use crate::protocol::discovery::OfferReceived;

pub fn generate_identity(name: &str) -> Identity {
    let random_node_id: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();

    Identity {
        name: name.to_string(),
        role: "manager".to_string(),
        identity: NodeId::from(random_node_id.as_bytes()),
    }
}

#[allow(unused)]
pub fn sample_offer_received() -> OfferReceived {
    OfferReceived {
        offer: sample_offer(),
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
    use ya_client::model::market::{Demand, Offer};

    #[allow(unused)]
    pub fn sample_offer() -> Offer {
        Offer::new(
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

    #[allow(unused)]
    pub fn sample_demand() -> Demand {
        Demand::new(
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
}
