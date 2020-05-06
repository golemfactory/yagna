use diesel::prelude::*;

use crate::db::schema::market_offer;


#[derive(Debug, Identifiable, Insertable)]
#[table_name = "market_offer"]
pub struct Offer {
    pub id: String,
    pub properties: String,
    pub constraints: String,
    pub node_id: Option<String>,
}

