use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum EventType {
    #[serde(rename = "RECEIVED")]
    Received,
    #[serde(rename = "ACCEPTED")]
    Accepted,
    #[serde(rename = "REJECTED")]
    Rejected,
    #[serde(rename = "CANCELLED")]
    Cancelled,
}
