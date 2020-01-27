use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Allocation {
    #[serde(rename = "allocationId")]
    pub allocation_id: String,
    #[serde(rename = "totalAmount")]
    pub total_amount: i32,
    #[serde(rename = "spentAmount")]
    pub spent_amount: i32,
    #[serde(rename = "remainingAmount")]
    pub remaining_amount: i32,
    #[serde(rename = "timeout", skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(rename = "makeDeposit")]
    pub make_deposit: bool,
}

impl Allocation {
    pub fn new(
        allocation_id: String,
        total_amount: i32,
        spent_amount: i32,
        remaining_amount: i32,
        make_deposit: bool,
    ) -> Allocation {
        Allocation {
            allocation_id,
            total_amount,
            spent_amount,
            remaining_amount,
            timeout: None,
            make_deposit,
        }
    }
}
