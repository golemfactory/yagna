use diesel::prelude::*;
use serde_json;
use uuid::Uuid;

use ya_client::model::market::Demand as ClientDemand;
use ya_client::model::ErrorMessage;
use ya_service_api_web::middleware::Identity;

use crate::db::schema::market_demand;
use crate::db::models::offer::generate_subscription_id;

#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_demand"]
pub struct Demand {
    pub id: String,
    pub properties: String,
    pub constraints: String,
    pub node_id: String,
}


impl Demand {
    pub fn from(demand: &ClientDemand) -> Result<Demand, ErrorMessage> {
        Ok(Demand {
            id: demand.demand_id.clone().unwrap_or(generate_subscription_id()),
            properties: demand.properties.to_string(),
            constraints: demand.constraints.clone(),
            node_id: demand
                .requestor_id()
                .map_err(|error| format!("Anonymous demand - {}", error))?
                .to_string(),
        })
    }

    pub fn from_with_identity(demand: &ClientDemand, id: &Identity) -> Demand {
        Demand {
            id: demand.demand_id.clone().unwrap_or(generate_subscription_id()),
            properties: demand.properties.to_string(),
            constraints: demand.constraints.clone(),
            node_id: id.identity.to_string(),
        }
    }

    pub fn into_client_offer(&self) -> Result<ClientDemand, ErrorMessage> {
        Ok(ClientDemand {
            demand_id: Some(self.id.clone()),
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
