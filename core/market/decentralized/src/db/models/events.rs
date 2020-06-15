use chrono::NaiveDateTime;

use super::SubscriptionId;


pub enum EventType {
    Provider(ProviderEventType),
    Requestor(RequestorEventType),
}

pub enum ProviderEventType {
    Proposal = 1001,
    Agreement = 1002,
    PropertyQuery = 1003,
}

pub enum RequestorEventType {
    Proposal = 2001,
    PropertyQuery = 2002,
}

/// TODO: We need two separate tables for Provider and Requestor events.
///  This way we can avoid storing additional field with flag.
pub struct MarketEvent {
    pub subscription_id: SubscriptionId,
    pub timestamp: NaiveDateTime,
    pub event_type: EventType,
    /// It can be Proposal, Agreement or structure,
    /// that will represent PropertyQuery.
    pub artifact_id: String,
}


