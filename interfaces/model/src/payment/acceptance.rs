use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Acceptance {
    #[serde(rename = "totalAmountAccepted")]
    pub total_amount_accepted: i32,
    #[serde(rename = "allocationId", skip_serializing_if = "Option::is_none")]
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
