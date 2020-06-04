use chrono::{NaiveDateTime, TimeZone, Utc};
use diesel::prelude::*;
use serde_json;
use std::str::FromStr;
use uuid::Uuid;

use ya_client::model::market::Demand as ClientDemand;
use ya_client::model::ErrorMessage;
use ya_service_api_web::middleware::Identity;

use super::SubscriptionId;
use crate::db::schema::market_demand;

#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_demand"]
pub struct Demand {
    pub id: SubscriptionId,
    pub properties: String,
    pub constraints: String,
    pub node_id: String,

    /// Creation time of Demand on Requestor side.
    pub creation_time: NaiveDateTime,
    /// Time when this Demand was added to our local database.
    pub addition_time: NaiveDateTime,
    /// Time when Demand expires set by Requestor.
    pub expiration_time: NaiveDateTime,
}

impl Demand {
    pub fn from(demand: &ClientDemand) -> Result<Demand, ErrorMessage> {
        let properties = demand.properties.to_string();
        let constraints = demand.constraints.clone();
        let node_id = demand.requestor_id()?.to_string();
        let id = SubscriptionId::from_str(demand.demand_id()?)?;

        // TODO: Set default expiration time. In future provider should set expiration.
        // TODO: Creation time should come from ClientOffer
        // TODO: Creation time should be included in subscription id hash.
        let creation_time = Utc::now().naive_utc();

        Ok(Demand {
            id,
            properties,
            constraints,
            node_id,
            creation_time: creation_time.clone(),
            addition_time: creation_time.clone(),
            expiration_time: creation_time.clone(),
        })
    }

    pub fn from_new(demand: &ClientDemand, id: &Identity) -> Demand {
        let properties = demand.properties.to_string();
        let constraints = demand.constraints.clone();
        let node_id = id.identity.to_string();
        let id = SubscriptionId::generate_id(&properties, &constraints, &node_id);

        // TODO: Set default expiration time. In future provider should set expiration.
        // TODO: Creation time should be included in subscription id hash.
        // This function creates new Demand, so creation time should be equal to addition time.
        let creation_time = Utc::now().naive_utc();

        Demand {
            id,
            properties,
            constraints,
            node_id,
            creation_time: creation_time.clone(),
            addition_time: creation_time.clone(),
            expiration_time: creation_time.clone(),
        }
    }

    pub fn into_client_offer(&self) -> Result<ClientDemand, ErrorMessage> {
        Ok(ClientDemand {
            demand_id: Some(self.id.to_string()),
            requestor_id: Some(self.node_id.clone()),
            constraints: self.constraints.clone(),
            properties: serde_json::from_str(&self.properties).map_err(|error| {
                format!(
                    "Can't serialize Demand properties from database!!! Error: {}",
                    error
                )
            })?,
        })
    }
}
