#![allow(clippy::all)]

use super::schema::*;
use chrono::NaiveDateTime;
use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql};
use diesel::serialize::{Output, ToSql};
use diesel::sql_types::Integer;
use std::convert::TryFrom;

#[derive(Queryable, Debug, Identifiable)]
#[diesel(table_name = activity)]
pub struct Activity {
    pub id: i32,
    pub natural_id: String,
    pub agreement_id: String,
    pub state_id: i32,
    pub usage_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[diesel(table_name = activity_event)]
pub struct ActivityEvent {
    pub id: i32,
    pub activity_id: i32,
    pub identity_id: String,
    pub event_date: NaiveDateTime,
    pub event_type_id: ActivityEventType,
}

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[diesel(sql_type = Integer)]
pub enum ActivityEventType {
    CreateActivity = 1,
    DestroyActivity = 2,
}

impl<DB: Backend> ToSql<Integer, DB> for ActivityEventType
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, DB>) -> diesel::serialize::Result {
        let value = match self {
            Self::CreateActivity => &1,
            Self::DestroyActivity => &2,
        };
        <i32 as ToSql<Integer, DB>>::to_sql(value, out)
    }
}

impl<DB> FromSql<Integer, DB> for ActivityEventType
where
    i32: FromSql<Integer, DB>,
    DB: Backend,
{
    fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
        Ok(match <i32 as FromSql<Integer, DB>>::from_sql(bytes)? {
            1 => ActivityEventType::CreateActivity,
            2 => ActivityEventType::DestroyActivity,
            _ => return Err(anyhow::anyhow!("invalid value").into()),
        })
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[diesel(table_name = activity_state)]
pub struct ActivityState {
    pub id: i32,
    pub name: String,
    pub reason: Option<String>,
    pub error_message: Option<String>,
    pub updated_date: NaiveDateTime,
}

impl TryFrom<ActivityState> for ya_client_model::activity::ActivityState {
    type Error = ya_persistence::Error;

    fn try_from(value: ActivityState) -> Result<Self, Self::Error> {
        Ok(ya_client_model::activity::ActivityState {
            state: serde_json::from_str(&value.name)?,
            reason: value.reason,
            error_message: value.error_message,
        })
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[diesel(table_name = activity_usage)]
pub struct ActivityUsage {
    pub id: i32,
    pub vector_json: Option<String>,
    pub updated_date: NaiveDateTime,
}

impl TryFrom<ActivityUsage> for ya_client_model::activity::ActivityUsage {
    type Error = ya_persistence::Error;

    fn try_from(value: ActivityUsage) -> Result<Self, Self::Error> {
        Ok(ya_client_model::activity::ActivityUsage {
            current_usage: value
                .vector_json
                .map(|json_str| serde_json::from_str(&json_str))
                .transpose()?,
            timestamp: value.updated_date.and_utc().timestamp(),
        })
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[diesel(table_name = runtime_event)]
pub struct RuntimeEvent {
    pub id: i32,
    pub activity_id: i32,
    pub batch_id: String,
    pub index: i32,
    pub timestamp: NaiveDateTime,
    pub type_id: RuntimeEventType,
    pub command: Option<String>,
    pub return_code: Option<i32>,
    pub message: Option<String>,
}

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[diesel(sql_type = Integer)]
pub enum RuntimeEventType {
    Started = 1,
    Finished = 2,
    StdOut = 3,
    StdErr = 4,
}

impl<DB: Backend> ToSql<Integer, DB> for RuntimeEventType
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, DB>) -> diesel::serialize::Result {
        let value = match self {
            Self::Started => &1,
            Self::Finished => &2,
            Self::StdOut => &3,
            Self::StdErr => &4,
        };
        <i32 as ToSql<Integer, DB>>::to_sql(value, out)
    }
}

impl<DB> FromSql<Integer, DB> for RuntimeEventType
where
    i32: FromSql<Integer, DB>,
    DB: Backend,
{
    fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
        Ok(match <i32 as FromSql<Integer, DB>>::from_sql(bytes)? {
            1 => RuntimeEventType::Started,
            2 => RuntimeEventType::Finished,
            3 => RuntimeEventType::StdOut,
            4 => RuntimeEventType::StdErr,
            _ => return Err(anyhow::anyhow!("invalid value").into()),
        })
    }
}

#[derive(Queryable, Debug, Clone, Identifiable, Insertable, AsChangeset)]
#[diesel(table_name = activity_credentials)]
#[diesel(primary_key(activity_id))]
pub struct ActivityCredentials {
    pub activity_id: String,
    pub credentials: String,
}

impl TryFrom<ActivityCredentials> for Option<ya_client_model::activity::Credentials> {
    type Error = ya_persistence::Error;

    fn try_from(value: ActivityCredentials) -> Result<Self, Self::Error> {
        Ok(serde_json::from_str(&value.credentials)?)
    }
}
