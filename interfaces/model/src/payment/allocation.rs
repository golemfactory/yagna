use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Allocation {
    pub allocation_id: String,
    pub total_amount: i32,
    pub spent_amount: i32,
    pub remaining_amount: i32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timeout: Option<String>,
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
