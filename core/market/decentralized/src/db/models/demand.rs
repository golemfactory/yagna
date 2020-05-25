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
}

impl Demand {
    pub fn from(demand: &ClientDemand) -> Result<Demand, ErrorMessage> {
        let properties = demand.properties.to_string();
        let constraints = demand.constraints.clone();
        let node_id = demand.requestor_id()?.to_string();
        let id = SubscriptionId::from_str(demand.demand_id()?)?;

        Ok(Demand {
            id,
            properties,
            constraints,
            node_id,
        })
    }

    pub fn from_new(demand: &ClientDemand, id: &Identity) -> Demand {
        let properties = demand.properties.to_string();
        let constraints = demand.constraints.clone();
        let node_id = id.identity.to_string();
        let id = SubscriptionId::generate_id(&properties, &constraints, &node_id);

        Demand {
            id,
            properties,
            constraints,
            node_id,
        }
    }

    pub fn into_client_offer(&self) -> Result<ClientDemand, ErrorMessage> {
        Ok(ClientDemand {
            demand_id: Some(self.id.to_string()),
            requestor_id: Some(self.node_id.clone()),
            constraints: self.constraints.clone(),
            properties: serde_json::to_value(&self.properties).map_err(|error| {
                format!(
                    "Can't serialize Demand properties from database!!! Error: {}",
                    error
                )
            })?,
        })
    }
}
