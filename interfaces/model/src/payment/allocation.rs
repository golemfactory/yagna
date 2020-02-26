use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Allocation {
    pub allocation_id: String,
    pub total_amount: BigDecimal,
    pub spent_amount: BigDecimal,
    pub remaining_amount: BigDecimal,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timeout: Option<DateTime<Utc>>,
    pub make_deposit: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAllocation {
    pub total_amount: BigDecimal,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timeout: Option<DateTime<Utc>>,
    pub make_deposit: bool,
}

impl Allocation {
    pub fn new(
        allocation_id: String,
        total_amount: BigDecimal,
        spent_amount: BigDecimal,
        remaining_amount: BigDecimal,
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
