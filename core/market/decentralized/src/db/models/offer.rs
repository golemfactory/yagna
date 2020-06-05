use chrono::{Duration, NaiveDateTime, TimeZone, Utc};
use diesel::prelude::*;
use serde_json;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use ya_client::model::market::Offer as ClientOffer;
use ya_client::model::ErrorMessage;
use ya_service_api_web::middleware::Identity;

use super::SubscriptionId;
use crate::db::schema::{market_offer, market_offer_unsubscribed};

#[derive(Clone, Debug, Identifiable, Insertable, Queryable, Deserialize, Serialize)]
#[table_name = "market_offer"]
pub struct Offer {
    pub id: SubscriptionId,
    pub properties: String,
    pub constraints: String,
    pub node_id: String,

    /// Creation time of Offer on Provider side.
    pub creation_ts: NaiveDateTime,
    /// Timestamp of adding this Offer to database.
    pub insertion_ts: Option<NaiveDateTime>,
    /// Time when Offer expires set by Provider.
    pub expiration_ts: NaiveDateTime,
}

#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_offer_unsubscribed"]
pub struct OfferUnsubscribed {
    pub id: SubscriptionId,
    pub timestamp: NaiveDateTime,
    pub node_id: String,
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

        id.validate(&properties, &constraints, &node_id)?;

        // TODO: Set default expiration time. In future provider should set expiration.
        // TODO: Creation time should come from ClientOffer
        // TODO: Creation time should be included in subscription id hash.
        let creation_ts = Utc::now().naive_utc();
        let expiration_ts = creation_ts + Duration::hours(24);

        Ok(Offer {
            id,
            properties,
            constraints,
            node_id,
            creation_ts,
            insertion_ts: None, // Database will insert this timestamp.
            expiration_ts,
        })
    }

    /// Creates new model offer. If ClientOffer has id already assigned,
    /// it will be ignored and regenerated.
    pub fn from_new(offer: &ClientOffer, id: &Identity) -> Offer {
        let properties = offer.properties.to_string();
        let constraints = offer.constraints.clone();
        let node_id = id.identity.to_string();
        let id = SubscriptionId::generate_id(&properties, &constraints, &node_id);

        // TODO: Set default expiration time. In future provider should set expiration.
        // TODO: Creation time should be included in subscription id hash.
        // This function creates new Offer, so creation time should be equal to addition time.
        let creation_ts = Utc::now().naive_utc();
        let expiration_ts = creation_ts + Duration::hours(24);

        Offer {
            id,
            properties,
            constraints,
            node_id,
            creation_ts,
            insertion_ts: None, // Database will insert this timestamp.
            expiration_ts,
        }
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

    pub fn validate(&self) -> Result<(), ErrorMessage> {
        Ok(self.id.validate(&self.properties, &self.constraints, &self.node_id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use ya_client::model::NodeId;

    #[test]
    // Offer with subscription id, that has wrong hash, should fail to create model.
    fn test_offer_validation_wrong_hash() {
        let false_subscription_id = "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53";
        let node_id = "12412abcf3112412abcf".to_string();

        let offer = ClientOffer {
            offer_id: Some(false_subscription_id.to_string()),
            properties: json!({}),
            constraints: "()".to_string(),
            provider_id: Some(NodeId::from(node_id[..].as_bytes()).to_string()),
        };

        assert!(Offer::from(&offer).is_err());
    }

    #[test]
    fn test_offer_validation_good_hash() {
        let subscription_id = "c76161077d0343ab85ac986eb5f6ea38-df6f6ad8c04d6bbc9dbfe87a5964b8be3c01e8456b1bc2d5d78fd4ef6851b071";
        let node_id = "12412abcf3112412abcf".to_string();

        let offer = ClientOffer {
            offer_id: Some(subscription_id.to_string()),
            properties: json!({}),
            constraints: "()".to_string(),
            provider_id: Some(NodeId::from(node_id[..].as_bytes()).to_string()),
        };

        assert!(Offer::from(&offer).is_ok());
    }
}
