#![allow(unused)]
#![allow(clippy::all)]

use crate::db::schema::*;
use chrono::NaiveDateTime;

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "app_key"]
pub struct AppKey {
    pub id: i32,
    pub role_id: i32,
    pub name: String,
    pub key: String,
    pub identity: String,
    pub created_date: NaiveDateTime,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "role"]
pub struct Role {
    pub id: i32,
    pub name: String,
}
