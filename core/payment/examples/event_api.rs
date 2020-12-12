use serde::{Deserialize, Serialize};
use serde_json; // 1.0.60
use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;

use ya_client_model::payment::{Rejection,InvoiceEvent,EventType, RejectionReason};


fn main() {
    let rejection = Rejection {
        rejection_reason: RejectionReason::UnsolicitedService,
        total_amount_accepted: BigDecimal::from(1),
        message: None
    };
    let event = InvoiceEvent {
        invoice_id: "ID".to_string(),
        event_date: Utc::now(),
        event_type: EventType::Rejected{ rejection }
    };
    let serialized = serde_json::to_string(&event).unwrap();
    println!("{}", serialized);
    let deserialized: InvoiceEvent = serde_json::from_str(&serialized).unwrap();
    println!("{:?}", deserialized);
}
