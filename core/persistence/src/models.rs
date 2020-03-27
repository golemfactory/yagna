#![allow(unused)]
#![allow(clippy::all)]

use crate::schema::*;
use chrono::NaiveDateTime;
use diesel::backend::Backend;
use diesel::deserialize;
use diesel::serialize::Output;
use diesel::sql_types::Integer;
use diesel::types::{FromSql, ToSql};
use std::convert::TryFrom;
use std::error::Error;

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
    pub identity_id: String,
    pub event_date: NaiveDateTime,
    pub event_type_id: ActivityEventType,
}

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[sql_type = "Integer"]
pub enum ActivityEventType {
    CreateActivity = 1,
    DestroyActivity = 2,
}

impl<DB: Backend> ToSql<Integer, DB> for ActivityEventType
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<W: std::io::Write>(&self, out: &mut Output<W, DB>) -> diesel::serialize::Result {
        (*self as i32).to_sql(out)
    }
}

impl<DB> FromSql<Integer, DB> for ActivityEventType
where
    i32: FromSql<Integer, DB>,
    DB: Backend,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        Ok(match i32::from_sql(bytes)? {
            1 => ActivityEventType::CreateActivity,
            2 => ActivityEventType::DestroyActivity,
            _ => return Err(anyhow::anyhow!("invalid value").into()),
        })
    }
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

impl std::convert::TryFrom<ActivityState> for ya_model::activity::ActivityState {
    type Error = crate::Error;

    fn try_from(value: ActivityState) -> Result<Self, Self::Error> {
        Ok(ya_model::activity::ActivityState {
            state: serde_json::from_str(&value.name)?,
            reason: value.reason,
            error_message: value.error_message,
        })
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity_usage"]
pub struct ActivityUsage {
    pub id: i32,
    pub vector_json: Option<String>,
    pub updated_date: NaiveDateTime,
}

impl std::convert::TryFrom<ActivityUsage> for ya_model::activity::ActivityUsage {
    type Error = crate::Error;

    fn try_from(value: ActivityUsage) -> Result<Self, Self::Error> {
        Ok(value
            .vector_json
            .map(|json_str| serde_json::from_str(&json_str))
            .transpose()?)
        .map(|current_usage| ya_model::activity::ActivityUsage { current_usage })
    }
}
