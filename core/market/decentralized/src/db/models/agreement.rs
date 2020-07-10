use chrono::{NaiveDateTime, Utc};
use diesel::backend::Backend;
use diesel::deserialize;
use diesel::serialize::Output;
use diesel::sql_types::Integer;
use diesel::types::{FromSql, ToSql};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use ya_client::model::NodeId;

use crate::db::models::{OwnerType, Proposal};
use crate::db::schema::market_agreement;
use crate::ProposalId;

pub type AgreementId = ProposalId;

/// TODO: Could we avoid having separate enum type for database
///  and separate for client?
#[derive(FromPrimitive, AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[sql_type = "Integer"]
pub enum AgreementState {
    /// Newly created by a Requestor (based on Proposal)
    Proposal = 0,
    /// Confirmed by a Requestor and send to Provider for approval
    Pending = 1,
    /// Cancelled by a Requestor
    Cancelled = 2,
    /// Rejected by a Provider
    Rejected = 3,
    /// Approved by both sides
    Approved = 4,
    /// Not accepted, rejected nor cancelled within validity period
    Expired = 5,
    /// Finished after approval
    Terminated = 6,
}

#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_agreement"]
pub struct Agreement {
    pub id: AgreementId,

    pub offer_properties: String,
    pub offer_constraints: String,

    pub demand_properties: String,
    pub demand_constraints: String,

    pub provider_id: NodeId,
    pub requestor_id: NodeId,

    /// End of validity period.
    /// Agreement needs to be accepted, rejected or cancelled before this date; otherwise will expire.
    pub creation_ts: NaiveDateTime,
    pub valid_to: NaiveDateTime,

    pub approved_date: Option<NaiveDateTime>,
    pub state: AgreementState,

    pub proposed_signature: Option<String>,
    pub approved_signature: Option<String>,
    pub committed_signature: Option<String>,
}

impl Agreement {
    pub fn new(
        demand_proposal: Proposal,
        offer_proposal: Proposal,
        valid_to: NaiveDateTime,
        owner: OwnerType,
    ) -> Agreement {
        let creation_ts = Utc::now().naive_utc();
        let agreement_id = ProposalId::generate_id(
            &offer_proposal.negotiation.offer_id,
            &offer_proposal.negotiation.demand_id,
            &creation_ts,
            owner,
        );

        Agreement {
            id: agreement_id,
            offer_properties: offer_proposal.body.properties,
            offer_constraints: offer_proposal.body.constraints,
            demand_properties: demand_proposal.body.properties,
            demand_constraints: demand_proposal.body.constraints,
            provider_id: demand_proposal.negotiation.provider_id,
            requestor_id: demand_proposal.negotiation.requestor_id,
            creation_ts,
            valid_to,
            approved_date: None,
            state: AgreementState::Proposal,
            proposed_signature: None,
            approved_signature: None,
            committed_signature: None,
        }
    }
}

impl<DB: Backend> ToSql<Integer, DB> for AgreementState
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<W: std::io::Write>(&self, out: &mut Output<W, DB>) -> diesel::serialize::Result {
        (*self as i32).to_sql(out)
    }
}

impl<DB> FromSql<Integer, DB> for AgreementState
where
    i32: FromSql<Integer, DB>,
    DB: Backend,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        let enum_value = i32::from_sql(bytes)?;
        Ok(FromPrimitive::from_i32(enum_value).ok_or(anyhow::anyhow!(
            "Invalid conversion from {} (i32) to Proposal State.",
            enum_value
        ))?)
    }
}
