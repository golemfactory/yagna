use std::collections::HashMap;
use std::convert::TryFrom;
use std::time::Duration;

use chrono::{DateTime, Utc};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::EnumMessage;
use strum_macros::*;

use crate::display::EnableDisplay;

use ya_client::model::market::Reason;
use ya_client::model::payment::{DebitNoteEventType, Rejection};

#[derive(Display, EnumMessage, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BreakReason {
    #[display(fmt = "Failed to initialize. Error: {}", error)]
    #[strum(message = "InitializationError")]
    InitializationError { error: String },
    #[display(fmt = "Agreement expired @ {}", _0)]
    #[strum(message = "Expired")]
    Expired(DateTime<Utc>),
    #[display(fmt = "No activity created within {:?}", _0)]
    #[strum(message = "NoActivity")]
    NoActivity(Duration),
    #[display(
        fmt = "Requestor isn't accepting DebitNotes in time ({})",
        "_0.display()"
    )]
    #[strum(message = "DebitNotesDeadline")]
    DebitNotesDeadline(chrono::Duration),
    #[display(fmt = "Requestor rejected the debit note: {:?}", _0)]
    #[strum(message = "DebitNoteRejected")]
    DebitNoteRejected(Rejection),
    #[display(fmt = "Provider cancelled the debit note")]
    #[strum(message = "DebitNoteCancelled")]
    DebitNoteCancelled,
    #[display(
        fmt = "Requestor didn't pay for DebitNote in time ({})",
        "_0.display()"
    )]
    #[strum(message = "DebitNoteNotPaid")]
    DebitNoteNotPaid(chrono::Duration),
    #[display(fmt = "Requestor is unreachable more than {}", "_0.display()")]
    #[strum(message = "RequestorUnreachable")]
    RequestorUnreachable(chrono::Duration),
}

impl TryFrom<DebitNoteEventType> for BreakReason {
    type Error = ();

    fn try_from(event: DebitNoteEventType) -> Result<Self, Self::Error> {
        match event {
            DebitNoteEventType::DebitNoteRejectedEvent { rejection } => {
                Ok(Self::DebitNoteRejected(rejection))
            }
            DebitNoteEventType::DebitNoteCancelledEvent => Ok(Self::DebitNoteCancelled),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GolemReason {
    #[serde(rename = "message")]
    pub message: String,
    #[serde(rename = "golem.provider.code")]
    pub code: String,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl GolemReason {
    pub fn new(reason: &BreakReason) -> GolemReason {
        GolemReason {
            message: reason.to_string(),
            code: reason.get_message().unwrap_or("Unknown").to_string(),
            extra: HashMap::new(),
        }
    }

    pub fn success() -> GolemReason {
        GolemReason {
            message: "Finished with success.".to_string(),
            code: "Success".to_string(),
            extra: HashMap::new(),
        }
    }

    pub fn to_client(&self) -> Option<Reason> {
        match Reason::from_value(self) {
            Ok(r) => Some(r),
            Err(e) => {
                log::warn!("{}", e);
                None
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_try_convert_self() {
        let g = GolemReason::success();
        let g1: GolemReason = g.to_client().unwrap().to_value().unwrap();
        assert_eq!(g, g1)
    }
}
