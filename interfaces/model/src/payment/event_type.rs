use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EventType {
    Received,
    Accepted,
    Rejected,
    Cancelled,
}

impl From<String> for EventType {
    fn from(value: String) -> Self {
        serde_json::from_str(&format!("\"{}\"", value)).unwrap()
    }
}

impl From<EventType> for String {
    fn from(event_type: EventType) -> Self {
        serde_json::to_string(&event_type)
            .unwrap()
            .trim_matches('"')
            .to_owned()
    }
}
