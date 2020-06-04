use chrono::{NaiveDateTime, TimeZone, Utc};
use diesel::prelude::*;
use serde_json;
use std::str::FromStr;
use uuid::Uuid;

use ya_client::model::market::Offer as ClientOffer;
use ya_client::model::ErrorMessage;
use ya_service_api_web::middleware::Identity;

use super::SubscriptionId;
use crate::db::schema::market_offer;

#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_offer"]
pub struct Offer {
    pub id: SubscriptionId,
    pub properties: String,
    pub constraints: String,
    pub node_id: String,

    /// Creation time of Offer on Provider side.
    pub creation_time: NaiveDateTime,
    /// Time when this Offer was added to our local database.
    pub addition_time: NaiveDateTime,
    /// Time when Offer expires set by Provider.
    pub expiration_time: NaiveDateTime,
}

impl Offer {
    pub fn from(offer: &ClientOffer) -> Result<Offer, ErrorMessage> {
        let properties = offer.properties.to_string();
        let constraints = offer.constraints.clone();
        let node_id = offer
            .provider_id()
            .map_err(|error| format!("Anonymous offer - {}", error))?
            .to_string();
        let id = SubscriptionId::from_str(offer.offer_id()?)?;

        // TODO: Set default expiration time. In future provider should set expiration.
        // TODO: Creation time should come from ClientOffer
        // TODO: Creation time should be included in subscription id hash.
        let creation_time = Utc::now().naive_utc();

        Ok(Offer {
            id,
            properties,
            constraints,
            node_id,
            creation_time: creation_time.clone(),
            addition_time: creation_time.clone(),
            expiration_time: creation_time.clone(),
        })
    }

    pub fn from_new(offer: &ClientOffer, id: &Identity) -> Result<Offer, ErrorMessage> {
        let properties = offer.properties.to_string();
        let constraints = offer.constraints.clone();
        let node_id = id.identity.to_string();
        let id = SubscriptionId::generate_id(&properties, &constraints, &node_id);

        // TODO: Set default expiration time. In future provider should set expiration.
        // TODO: Creation time should be included in subscription id hash.
        // This function creates new Offer, so creation time should be equal to addition time.
        let creation_time = Utc::now().naive_utc();

        Ok(Offer {
            id,
            properties,
            constraints,
            node_id,
            creation_time: creation_time.clone(),
            addition_time: creation_time.clone(),
            expiration_time: creation_time.clone(),
        })
    }

    pub fn into_client_offer(&self) -> Result<ClientOffer, ErrorMessage> {
        Ok(ClientOffer {
            offer_id: Some(self.id.to_string()),
            provider_id: Some(self.node_id.clone()),
            constraints: self.constraints.clone(),
            properties: serde_json::from_str(&self.properties).map_err(|error| {
                format!(
                    "Can't serialize Offer properties from database!!! Error: {}",
                    error
                )
            })?,
        })
    }
}
