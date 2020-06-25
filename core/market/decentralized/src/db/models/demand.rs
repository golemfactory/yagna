use chrono::{Duration, NaiveDateTime, Utc};
use serde_json;

use ya_client::model::{market::Demand as ClientDemand, ErrorMessage, NodeId};
use ya_service_api_web::middleware::Identity;

use super::SubscriptionId;
use crate::db::schema::market_demand;

#[derive(Clone, Debug, Identifiable, Insertable, Queryable, PartialEq)]
#[table_name = "market_demand"]
pub struct Demand {
    pub id: SubscriptionId,
    pub properties: String,
    pub constraints: String,
    pub node_id: NodeId,

    /// Creation time of Demand on Requestor side.
    pub creation_ts: NaiveDateTime,
    /// Timestamp of adding this Demand to database.
    pub insertion_ts: Option<NaiveDateTime>,
    /// Time when Demand expires; set by Requestor.
    pub expiration_ts: NaiveDateTime,
}

impl Demand {
    /// Creates new model demand. If ClientDemand has id already assigned,
    /// it will be ignored and regenerated.
    pub fn from_new(demand: &ClientDemand, id: &Identity) -> Demand {
        let properties = demand.properties.to_string();
        let constraints = demand.constraints.clone();
        let node_id = id.identity;

        // TODO: Set default expiration time. In future provider should set expiration.
        // TODO: Creation time should be included in subscription id hash.
        // This function creates new Demand, so creation time should be equal to addition time.
        let creation_ts = Utc::now().naive_utc();
        let expiration_ts = creation_ts + Duration::hours(24);

        let id = SubscriptionId::generate_id(
            &properties,
            &constraints,
            &node_id,
            &creation_ts,
            &expiration_ts,
        );

        Demand {
            id,
            properties,
            constraints,
            node_id,
            creation_ts,
            insertion_ts: None, // Database will insert this timestamp.
            expiration_ts,
        }
    }

    pub fn into_client_demand(&self) -> Result<ClientDemand, ErrorMessage> {
        Ok(ClientDemand {
            demand_id: Some(self.id.to_string()),
            requestor_id: Some(self.node_id.to_string()), // TODO: use NodeId in client: issue #352
            constraints: self.constraints.clone(),
            properties: serde_json::from_str(&self.properties).map_err(|e| {
                format!(
                    "Can't serialize Demand properties from database. Error: {}",
                    e
                )
            })?,
        })
    }
}
