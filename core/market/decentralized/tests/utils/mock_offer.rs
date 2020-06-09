use std::string::ToString;
use ya_client::model::market::{Demand, Offer};

#[allow(unused)]
pub fn example_offer() -> Offer {
    let properties = serde_json::json!({
        "golem": {
            "node.id.name": "itstest".to_string(),
            "srv.comp.wasm.task_package": "test-package".to_string(),
        },
    });
    Offer::new(properties, "(golem.node.debug.subnet=blaa)".to_string())
}

#[allow(unused)]
pub fn example_demand() -> Demand {
    let properties = serde_json::json!({
        "golem": {
            "node.id.name": "itstest".to_string(),
            "srv.comp.wasm.task_package": "test-package".to_string(),
        },
    });
    Demand::new(properties, "(golem.node.debug.subnet=blaa)".to_string())
}
