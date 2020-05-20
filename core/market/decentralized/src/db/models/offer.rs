use diesel::prelude::*;
use serde_json;
use uuid::Uuid;

use ya_client::model::market::Offer as ClientOffer;
use ya_client::model::ErrorMessage;
use ya_service_api_web::middleware::Identity;

use crate::db::schema::market_offer;

#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_offer"]
pub struct Offer {
    pub id: String,
    pub properties: String,
    pub constraints: String,
    pub node_id: String,
}

/// TODO: Should be cryptographically strong.
pub fn generate_subscription_id() -> String {
    Uuid::new_v4().to_simple().to_string()
}

impl Offer {
    pub fn from(offer: &ClientOffer) -> Result<Offer, ErrorMessage> {
        Ok(Offer {
            id: offer.offer_id.clone().unwrap_or(generate_subscription_id()),
            properties: offer.properties.to_string(),
            constraints: offer.constraints.clone(),
            node_id: offer
                .provider_id()
                .map_err(|error| format!("Anonymous offer - {}", error))?
                .to_string(),
        })
    }

    pub fn from_with_identity(offer: &ClientOffer, id: &Identity) -> Offer {
        Offer {
            id: offer.offer_id.clone().unwrap_or(generate_subscription_id()),
            properties: offer.properties.to_string(),
            constraints: offer.constraints.clone(),
            node_id: id.identity.to_string(),
        }
    }

    pub fn into_client_offer(&self) -> Result<ClientOffer, ErrorMessage> {
        Ok(ClientOffer {
            offer_id: Some(self.id.clone()),
            provider_id: Some(self.node_id.clone()),
            constraints: self.constraints.clone(),
            properties: serde_json::to_value(&self.properties).map_err(|error| {
                format!(
                    "Can't serialize Offer properties from database!!! Error: {}",
                    error
                )
            })?,
        })
    }
}
