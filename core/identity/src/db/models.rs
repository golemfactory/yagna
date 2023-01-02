#![allow(unused)]
#![allow(clippy::all)]

use crate::db::schema::{app_key, identity, role};
use chrono::NaiveDateTime;
use diesel::{Associations, Identifiable, Insertable, Queryable};
use ya_client_model::NodeId;

#[derive(Queryable, Debug, Identifiable, Insertable, Clone)]
#[table_name = "identity"]
#[primary_key(identity_id)]
pub struct Identity {
    pub identity_id: NodeId,
    pub key_file_json: String,
    pub is_default: bool,
    pub is_deleted: bool,
    pub alias: Option<String>,
    pub note: Option<String>,
    pub created_date: NaiveDateTime,
}

#[derive(Queryable, Debug, Associations, Identifiable)]
#[belongs_to(Identity)]
#[table_name = "app_key"]
pub struct AppKey {
    pub id: i32,
    pub role_id: i32,
    pub name: String,
    pub key: String,
    pub identity_id: NodeId,
    pub created_date: NaiveDateTime,
    pub allow_origin: Option<String>,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "role"]
pub struct Role {
    pub id: i32,
    pub name: String,
}

impl AppKey {
    pub fn to_core_model(self, role: Role) -> ya_core_model::appkey::AppKey {
        ya_core_model::appkey::AppKey {
            name: self.name,
            key: self.key,
            role: role.name,
            identity: self.identity_id,
            created_date: self.created_date,
            allow_origins: self
                .allow_origin
                .map(|origin| vec![origin])
                .unwrap_or(vec![]),
        }
    }
}
