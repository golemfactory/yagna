use chrono::{Duration, Utc};

use crate::db::model::{Demand, Offer};
use crate::protocol::discovery::GetOffers;
use crate::testing::mock_identity::generate_identity;

#[allow(unused)]
pub fn sample_get_offer_received() -> GetOffers {
    GetOffers {
        offers: vec![sample_offer().id],
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
