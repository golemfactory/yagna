use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Acceptance {
    pub total_amount_accepted: i32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub allocation_id: Option<String>,
}

impl Acceptance {
    pub fn new(total_amount_accepted: i32) -> Acceptance {
        Acceptance {
            total_amount_accepted,
            allocation_id: None,
        }
    }
}
