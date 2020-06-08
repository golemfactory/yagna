use chrono::{Duration, NaiveDateTime, TimeZone, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json;
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
    /// Creates new model offer. If ClientOffer has id already assigned,
    /// it will be ignored and regenerated.
    pub fn from_new(offer: &ClientOffer, id: &Identity) -> Offer {
        let properties = offer.properties.to_string();
        let constraints = offer.constraints.clone();
        let node_id = id.identity.to_string();

        // TODO: Set default expiration time. In future provider should set expiration.
        // This function creates new Offer, so creation time should be generated by database.
        let creation_ts = Utc::now().naive_utc();
        let expiration_ts = creation_ts + Duration::hours(24);

        let id = SubscriptionId::generate_id(
            &properties,
            &constraints,
            &node_id,
            &creation_ts,
            &expiration_ts,
        );

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
        Ok(self.id.validate(
            &self.properties,
            &self.constraints,
            &self.node_id,
            &self.creation_ts,
            &self.expiration_ts,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};
    use serde_json::json;
    use ya_client::model::NodeId;

    #[test]
    // Offer with subscription id, that has wrong hash, should fail to create model.
    fn test_offer_validation_wrong_hash() {
        let false_subscription_id = "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53";
        let node_id = "12412abcf3112412abcf".to_string();

        let offer = Offer {
            id: SubscriptionId::from_str(&false_subscription_id).unwrap(),
            properties: "{}".to_string(),
            constraints: "()".to_string(),
            node_id: NodeId::from(node_id[..].as_bytes()).to_string(),
            creation_ts: NaiveDateTime::new(
                NaiveDate::from_ymd(1970, 1, 1),
                NaiveTime::from_hms(0, 1, 1),
            ),
            insertion_ts: None,
            expiration_ts: NaiveDateTime::new(
                NaiveDate::from_ymd(1970, 1, 1),
                NaiveTime::from_hms(15, 1, 1),
            ),
        };
        assert!(offer.validate().is_err());
    }

    #[test]
    fn test_offer_validation_good_hash() {
        let subscription_id = "c76161077d0343ab85ac986eb5f6ea38-4f068a6ac3140bd1b00a44ddf2b61556ab4ab232201a4a957f1c7fbf191090b3";
        let node_id = "12412abcf3112412abcf".to_string();

        let offer = Offer {
            id: SubscriptionId::from_str(&subscription_id).unwrap(),
            properties: "{}".to_string(),
            constraints: "()".to_string(),
            node_id: NodeId::from(node_id[..].as_bytes()).to_string(),
            creation_ts: NaiveDateTime::new(
                NaiveDate::from_ymd(1970, 1, 1),
                NaiveTime::from_hms(0, 1, 1),
            ),
            insertion_ts: None,
            expiration_ts: NaiveDateTime::new(
                NaiveDate::from_ymd(1970, 1, 1),
                NaiveTime::from_hms(15, 1, 1),
            ),
        };
        println!("{}", offer.id.to_string());

        offer.validate().unwrap();
    }
}
