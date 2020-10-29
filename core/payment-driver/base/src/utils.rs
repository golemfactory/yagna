/*
    Common utility functions for dealing with PaymentDriver related objects
*/

// External crates
use chrono::{DateTime, Utc};

// Local uses
use crate::model::{PaymentDetails, SchedulePayment};

pub fn to_payment_details(msg: SchedulePayment, date: Option<DateTime<Utc>>) -> PaymentDetails {
    PaymentDetails {
        recipient: msg.recipient().to_string(),
        sender: msg.sender().to_string(),
        amount: msg.amount(),
        date,
    }
}
