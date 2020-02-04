use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejection {
    pub rejection_reason: crate::payment::RejectionReason,
    pub total_amount_accepted: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Rejection {
    pub fn new(
        rejection_reason: crate::payment::RejectionReason,
        total_amount_accepted: i32,
    ) -> Rejection {
        Rejection {
            rejection_reason,
            total_amount_accepted,
            message: None,
        }
    }
}
