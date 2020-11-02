use chrono::NaiveDateTime;
use serde_json;

use ya_client::model::{market::Demand as ClientDemand, ErrorMessage, NodeId};
use ya_market_resolver::flatten::{flatten_json, JsonObjectExpected};
use ya_service_api_web::middleware::Identity;

use super::SubscriptionId;
use crate::db::schema::market_demand;

#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
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
    pub fn from_new(
        demand: &ClientDemand,
        id: &Identity,
        creation_ts: NaiveDateTime,
        expiration_ts: NaiveDateTime,
    ) -> Result<Demand, JsonObjectExpected> {
        let properties = flatten_json(&demand.properties)?.to_string();
        let constraints = demand.constraints.clone();
        let node_id = id.identity;

        let id = SubscriptionId::generate_id(
            &properties,
            &constraints,
            &node_id,
            &creation_ts,
            &expiration_ts,
        );

        Ok(Demand {
            id,
            properties,
            constraints,
            node_id,
            creation_ts,
            insertion_ts: None, // Database will insert this timestamp.
            expiration_ts,
        })
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

/// PartialEq implementation that ignores insertion_ts.
impl PartialEq for Demand {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.constraints == other.constraints
            && self.creation_ts == other.creation_ts
            && self.expiration_ts == other.expiration_ts
            && self.properties == other.properties
            && self.node_id == other.node_id
    }
}
