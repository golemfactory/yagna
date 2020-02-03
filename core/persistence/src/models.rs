#![allow(unused)]
#![allow(clippy::all)]

use crate::schema::*;
use chrono::NaiveDateTime;
use diesel::backend::Backend;
use diesel::serialize::{IsNull, Output, ToSql};
use diesel::sql_types::Integer;
use std::error::Error;

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "activity"]
pub struct Activity {
    pub id: i32,
    pub natural_id: String,
    pub agreement_id: i32,
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

#[derive(Insertable, Debug)]
#[table_name = "agreement"]
pub struct NewAgreement {
    pub natural_id: String,
    pub state_id: AgreementState,
    pub demand_node_id: String,
    pub demand_properties_json: String,
    pub demand_constraints_json: String,
    pub offer_node_id: String,
    pub offer_properties_json: String,
    pub offer_constraints_json: String,
    pub proposed_signature: String,
    pub approved_signature: String,
    pub committed_signature: Option<String>,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "agreement"]
pub struct Agreement {
    pub id: i32,
    pub natural_id: String,
    pub state_id: i32,
    pub demand_node_id: String,
    pub demand_properties_json: String,
    pub demand_constraints_json: String,
    pub offer_node_id: String,
    pub offer_properties_json: String,
    pub offer_constraints_json: String,
    pub proposed_signature: String,
    pub approved_signature: String,
    pub committed_signature: Option<String>,
}

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[sql_type = "Integer"]
pub enum AgreementState {
    New = 0,
    PendingApproval = 1,
    Approved = 10,
    Canceled = 40,
    Rejected = 41,
    Terminated = 50,
}

impl<DB: Backend> ToSql<Integer, DB> for AgreementState
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<W: std::io::Write>(&self, out: &mut Output<W, DB>) -> diesel::serialize::Result {
        (*self as i32).to_sql(out)
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "agreement_event"]
pub struct AgreementEvent {
    pub id: i32,
    pub agreement_id: i32,
    pub event_date: NaiveDateTime,
    pub event_type_id: i32,
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "agreement_event_type"]
pub struct AgreementEventType {
    pub id: i32,
    pub name: String,
}
