#![allow(unused)]
#![allow(clippy::all)]

use chrono::NaiveDateTime;
use std::error::Error;

use crate::schema::*;

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity"]
pub struct Activity {
    pub id: i32,
    pub natural_id: String,
    pub agreement_id: String,
    pub state_id: i32,
    pub usage_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_event"]
pub struct ActivityEvent {
    pub id: i32,
    pub activity_id: i32,
    pub event_date: NaiveDateTime,
    pub event_type_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_event_type"]
pub struct ActivityEventType {
    pub id: i32,
    pub name: String,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_state"]
pub struct ActivityState {
    pub id: i32,
    pub name: String,
    pub reason: Option<String>,
    pub error_message: Option<String>,
    pub updated_date: NaiveDateTime,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_usage"]
pub struct ActivityUsage {
    pub id: i32,
    pub vector_json: Option<String>,
    pub updated_date: NaiveDateTime,
}
