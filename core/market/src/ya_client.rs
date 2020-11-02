/// This file is temporary and will be replaced with Agreement Events implementation in ya-client.
/// Since we don't won't to make incompatible changes to ya-client, all client events will be defined here
/// until they can be moved.

mod model {
    mod market {
        mod event {

            use chrono::{DateTime, Utc};
            use serde::{Deserialize, Serialize};

            #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
            #[serde(tag = "eventType")]
            pub enum AgreementEvent {
                #[serde(rename = "AgreementApproved")]
                AgreementApproved {
                    #[serde(rename = "eventDate")]
                    event_date: DateTime<Utc>,
                },
                #[serde(rename = "AgreementRejected")]
                AgreementRejected {
                    #[serde(rename = "eventDate")]
                    event_date: DateTime<Utc>,
                },
                #[serde(rename = "AgreementCancelled")]
                AgreementCancelled {
                    #[serde(rename = "eventDate")]
                    event_date: DateTime<Utc>,
                },
                #[serde(rename = "AgreementTerminated")]
                AgreementTerminated {
                    #[serde(rename = "eventDate")]
                    event_date: DateTime<Utc>,
                },
            }
        }
    }
}
