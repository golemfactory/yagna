use chrono::NaiveDateTime;
use diesel::backend::Backend;
use diesel::deserialize;
use diesel::serialize::Output;
use diesel::sql_types::Integer;
use diesel::types::{FromSql, ToSql};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use crate::db::model::agreement::AppSessionId;
use crate::db::model::AgreementId;
use crate::db::schema::market_agreement_event;

#[derive(FromPrimitive, AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[sql_type = "Integer"]
pub enum AgreementEventType {
    AgreementApproved,
    AgreementRejected,
    AgreementCancelled,
    AgreementTimeout,
    AgreementTerminated,
}

#[derive(Clone, Debug, Queryable)]
pub struct AgreementEvent {
    pub id: i32,
    pub agreement_id: AgreementId,
    pub session_id: AppSessionId,
    pub event_type: AgreementEventType,
    pub timestamp: NaiveDateTime,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Insertable)]
#[table_name = "market_agreement_event"]
pub struct NewAgreementEvent {
    pub agreement_id: AgreementId,
    pub session_id: AppSessionId,
    pub event_type: AgreementEventType,
    pub reason: Option<String>,
}

/// TODO: Find way to implement this trait for all enums at once.
impl<DB: Backend> ToSql<Integer, DB> for AgreementEventType
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<W: std::io::Write>(&self, out: &mut Output<W, DB>) -> diesel::serialize::Result {
        (*self as i32).to_sql(out)
    }
}

impl<DB> FromSql<Integer, DB> for AgreementEventType
where
    i32: FromSql<Integer, DB>,
    DB: Backend,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        let enum_value = i32::from_sql(bytes)?;
        Ok(FromPrimitive::from_i32(enum_value).ok_or(anyhow::anyhow!(
            "Invalid conversion from {} (i32) to Agreement EventType.",
            enum_value
        ))?)
    }
}
