use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum InvoiceStatus {
    #[serde(rename = "ISSUED")]
    Issued,
    #[serde(rename = "RECEIVED")]
    Received,
    #[serde(rename = "ACCEPTED")]
    Accepted,
    #[serde(rename = "REJECTED")]
    Rejected,
    #[serde(rename = "FAILED")]
    Failes,
    #[serde(rename = "SETTLED")]
    Settled,
    #[serde(rename = "CANCELLED")]
    Cancelled,
}
