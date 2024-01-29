use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GsbHttpCallResponseEvent {
    pub index: usize,
    pub timestamp: String,
    pub msg: String,
}
