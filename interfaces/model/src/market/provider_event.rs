
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(tag = "eventType")]
pub enum ProviderEvent {
    #[serde(rename = "DemandEvent")]
    #[serde(rename_all = "camelCase")]
    Demand {
        requestor_id: String,
        demand: Option<crate::market::Proposal>,
    },
    #[serde(rename = "NewAgreementEvent")]
    #[serde(rename_all = "camelCase")]
    NewAgreement {
        requestor_id: String,
        agreement_id: Option<String>,
        demand: Option<crate::market::Demand>,
        provider_id: Option<String>,
        offer: Option<crate::market::Offer>,
    }
}
