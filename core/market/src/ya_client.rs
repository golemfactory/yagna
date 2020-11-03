/// This file is temporary and will be replaced with Agreement Events implementation in ya-client.
/// Since we don't won't to make incompatible changes to ya-client, all client events will be defined here
/// until they can be moved.

pub mod model {
    pub mod market {
        pub mod event {

            use chrono::{DateTime, Utc};
            use serde::{Deserialize, Serialize};

            #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
            #[serde(tag = "eventType")]
            pub enum AgreementEvent {
                #[serde(rename = "AgreementApprovedEvent")]
                AgreementApprovedEvent {
                    #[serde(rename = "eventDate")]
                    event_date: DateTime<Utc>,
                    #[serde(rename = "agreementId")]
                    agreement_id: String,
                },
                #[serde(rename = "AgreementRejectedEvent")]
                AgreementRejectedEvent {
                    #[serde(rename = "eventDate")]
                    event_date: DateTime<Utc>,
                    #[serde(rename = "agreementId")]
                    agreement_id: String,
                    #[serde(rename = "reason")]
                    reason: Option<String>,
                },
                #[serde(rename = "AgreementCancelledEvent")]
                AgreementCancelledEvent {
                    #[serde(rename = "eventDate")]
                    event_date: DateTime<Utc>,
                    #[serde(rename = "agreementId")]
                    agreement_id: String,
                    #[serde(rename = "reason")]
                    reason: Option<String>,
                },
                #[serde(rename = "AgreementTerminatedEvent")]
                AgreementTerminatedEvent {
                    #[serde(rename = "eventDate")]
                    event_date: DateTime<Utc>,
                    #[serde(rename = "agreementId")]
                    agreement_id: String,
                    #[serde(rename = "reason")]
                    reason: Option<String>,
                },
            }
        }
    }
}
