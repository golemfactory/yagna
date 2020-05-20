use diesel::prelude::*;
use uuid::Uuid;

use ya_client::model::market::Demand as ClientDemand;

use crate::db::schema::market_demand;

#[derive(Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_demand"]
pub struct Demand {
    pub id: String,
    pub properties: String,
    pub constraints: String,
    pub node_id: Option<String>,
}
