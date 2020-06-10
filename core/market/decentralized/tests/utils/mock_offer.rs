use std::string::ToString;

use ya_client::model::{
    market::{Demand, Offer},
    NodeId,
};
use ya_service_api_web::middleware::Identity;

#[allow(unused)]
pub fn example_offer() -> Offer {
    let properties = serde_json::json!({
        "golem": {
            "node.id.name": "itstest-provider",
            "node.debug.subnet": "blaa",
            "com.pricing.model": "linear"
        },
    });
    Offer::new(
        properties,
        "(&(golem.node.debug.subnet=blaa)(golem.srv.comp.expiration>0))".to_string(),
    )
}

#[allow(unused)]
pub fn example_demand() -> Demand {
    let properties = serde_json::json!({
        "golem": {
            "node.id.name": "itstest-requestor",
            "node.debug.subnet": "blaa",
            "srv.comp.expiration": 3,
            "srv.comp.wasm.task_package": "test-package",
        },
    });
    Demand::new(
        properties,
        "(&(golem.node.debug.subnet=blaa)(golem.com.pricing.model=linear))".to_string(),
    )
}

#[allow(unused)]
pub fn mock_id() -> Identity {
    Identity {
        identity: "0xbabe000000000000000000000000000000000000"
            .parse::<NodeId>()
            .unwrap(),
        name: "".to_string(),
        role: "".to_string(),
    }
}
