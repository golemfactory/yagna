use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rejection {
    #[serde(rename = "rejectionReason")]
    pub rejection_reason: crate::payment::RejectionReason,
    #[serde(rename = "totalAmountAccepted")]
    pub total_amount_accepted: i32,
    #[serde(rename = "message", skip_serializing_if = "Option::is_none")]
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
