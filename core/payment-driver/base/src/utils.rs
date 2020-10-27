/*
    Common utility functions for dealing with PaymentDriver related objects
*/

// External crates
use chrono::Utc;
use serde_json;

// Workspace uses
use ya_core_model::driver::{GenericError, PaymentConfirmation, PaymentDetails, SchedulePayment};

pub fn to_payment_details(msg: SchedulePayment) -> PaymentDetails {
    PaymentDetails {
        recipient: msg.recipient().to_string(),
        sender: msg.sender().to_string(),
        amount: msg.amount(),
        date: Some(Utc::now()),
    }
}

// Used by the DummyDriver to have a 2 way conversion between details & confirmation
pub fn to_confirmation(details: &PaymentDetails) -> Result<Vec<u8>, GenericError> {
    Ok(serde_json::to_string(details)
        .map_err(GenericError::new)?
        .into_bytes())
}

pub fn from_confirmation(
    confirmation: PaymentConfirmation,
) -> Result<PaymentDetails, GenericError> {
    let json_str =
        std::str::from_utf8(confirmation.confirmation.as_slice()).map_err(GenericError::new)?;
    let details = serde_json::from_str(&json_str).map_err(GenericError::new)?;
    Ok(details)
}
