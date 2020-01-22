use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum RejectionReason {
    #[serde(rename = "UNSOLICITED_SERVICE")]
    UnsolicitedService,
    #[serde(rename = "BAD_SERVICE")]
    BadService,
    #[serde(rename = "INCORRECT_AMOUNT")]
    IncorrectAmount,
}
