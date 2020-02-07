use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::{
    backend::Backend,
    serialize::{Output, ToSql},
    sql_types::Integer,
};
use std::convert::TryFrom;

use ya_model::market::agreement::{Agreement as ApiAgreement, State as ApiAgreementState};

use crate::db::schema::agreement;
use ya_model::market::{Demand, Offer};

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "agreement"]
pub struct Agreement {
    pub id: i32,
    pub natural_id: String,
    pub state_id: i32,
    pub demand_node_id: String,
    pub demand_properties_json: String,
    pub demand_constraints: String,
    pub offer_node_id: String,
    pub offer_properties_json: String,
    pub offer_constraints: String,
    pub valid_to: NaiveDateTime,
    pub approved_date: Option<NaiveDateTime>,
    pub proposed_signature: String,
    pub approved_signature: String,
    pub committed_signature: Option<String>,
}

#[derive(Insertable, Debug)]
#[table_name = "agreement"]
pub struct NewAgreement {
    pub natural_id: String,
    pub state_id: AgreementState,
    pub demand_node_id: String,
    pub demand_properties_json: String,
    pub demand_constraints: String,
    pub offer_node_id: String,
    pub offer_properties_json: String,
    pub offer_constraints: String,
    pub valid_to: NaiveDateTime,
    pub approved_date: Option<NaiveDateTime>,
    pub proposed_signature: String,
    pub approved_signature: String,
    pub committed_signature: Option<String>,
}

impl TryFrom<ApiAgreement> for NewAgreement {
    type Error = ConversionError;

    fn try_from(agreement: ApiAgreement) -> Result<Self, Self::Error> {
        Ok(NewAgreement {
            natural_id: agreement.agreement_id,
            state_id: agreement.state.into(),
            demand_node_id: agreement
                .demand
                .requestor_id
                .ok_or(ConversionError::NoRequestorId)?,
            demand_properties_json: serde_json::to_string_pretty(&agreement.demand.properties)?,
            demand_constraints: agreement.demand.constraints,
            offer_node_id: agreement
                .offer
                .provider_id
                .ok_or(ConversionError::NoProviderId)?,
            offer_properties_json: serde_json::to_string_pretty(&agreement.offer.properties)?,
            offer_constraints: agreement.offer.constraints,
            valid_to: agreement.valid_to.naive_utc(),
            approved_date: agreement.approved_date.map(|ad| ad.naive_utc()),
            proposed_signature: agreement.proposed_signature.unwrap_or_default(),
            approved_signature: agreement.approved_signature.unwrap_or_default(),
            committed_signature: agreement.committed_signature,
        })
    }
}

impl TryFrom<Agreement> for ApiAgreement {
    type Error = ConversionError;

    fn try_from(agreement: Agreement) -> Result<Self, Self::Error> {
        Ok(ApiAgreement {
            agreement_id: agreement.natural_id,
            demand: Demand {
                properties: agreement.demand_properties_json.parse()?,
                constraints: agreement.demand_constraints,
                demand_id: None,
                requestor_id: Some(agreement.demand_node_id),
            },
            offer: Offer {
                properties: agreement.offer_properties_json.parse()?,
                constraints: agreement.offer_constraints,
                offer_id: None,
                provider_id: Some(agreement.offer_node_id),
            },
            valid_to: DateTime::from_utc(agreement.valid_to, Utc),
            approved_date: agreement
                .approved_date
                .map(|ad| DateTime::from_utc(ad, Utc)),
            state: AgreementState::try_from(agreement.state_id)?.into(),
            proposed_signature: None,
            approved_signature: None,
            committed_signature: None,
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ConversionError {
    #[error("no requestor id")]
    NoRequestorId,
    #[error("no provider id")]
    NoProviderId,
    #[error("serde JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no such state: {0}")]
    NonExistentState(i32),
}

#[derive(AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[sql_type = "Integer"]
pub enum AgreementState {
    /// new
    Proposal = 0,
    Pending = 1,
    Approved = 10,
    Cancelled = 40,
    Rejected = 41,
    Expired = 42,
    Terminated = 50,
}

impl From<ApiAgreementState> for AgreementState {
    fn from(model: ApiAgreementState) -> Self {
        match model {
            ApiAgreementState::Proposal => AgreementState::Proposal,
            ApiAgreementState::Pending => AgreementState::Pending,
            ApiAgreementState::Cancelled => AgreementState::Cancelled,
            ApiAgreementState::Rejected => AgreementState::Rejected,
            ApiAgreementState::Approved => AgreementState::Approved,
            ApiAgreementState::Expired => AgreementState::Expired,
            ApiAgreementState::Terminated => AgreementState::Terminated,
        }
    }
}

impl From<AgreementState> for ApiAgreementState {
    fn from(model: AgreementState) -> Self {
        match model {
            AgreementState::Proposal => ApiAgreementState::Proposal,
            AgreementState::Pending => ApiAgreementState::Pending,
            AgreementState::Cancelled => ApiAgreementState::Cancelled,
            AgreementState::Rejected => ApiAgreementState::Rejected,
            AgreementState::Approved => ApiAgreementState::Approved,
            AgreementState::Expired => ApiAgreementState::Expired,
            AgreementState::Terminated => ApiAgreementState::Terminated,
        }
    }
}

impl TryFrom<i32> for AgreementState {
    type Error = ConversionError;

    fn try_from(state_id: i32) -> Result<Self, Self::Error> {
        match state_id {
            0 => Ok(AgreementState::Proposal),
            1 => Ok(AgreementState::Pending),
            10 => Ok(AgreementState::Approved),
            40 => Ok(AgreementState::Cancelled),
            41 => Ok(AgreementState::Rejected),
            42 => Ok(AgreementState::Expired),
            50 => Ok(AgreementState::Terminated),
            id => Err(ConversionError::NonExistentState(id)),
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

//#[derive(Queryable, Debug, Identifiable)]
//#[table_name = "agreement_event"]
//pub struct AgreementEvent {
//    pub id: i32,
//    pub agreement_id: i32,
//    pub event_date: NaiveDateTime,
//    pub event_type_id: i32,
//}
//
//#[derive(Queryable, Debug, Identifiable)]
//#[table_name = "agreement_event_type"]
//pub struct AgreementEventType {
//    pub id: i32,
//    pub name: String,
//}
