use chrono::{Duration, NaiveDateTime, Utc};
use diesel::backend::Backend;
use diesel::deserialize;
use diesel::serialize::Output;
use diesel::sql_types::Integer;
use diesel::types::{FromSql, ToSql};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use ya_client::model::market::proposal::{Proposal as ClientProposal, State};
use ya_client::model::{ErrorMessage, NodeId};

use super::{generate_random_id, SubscriptionId};
use super::{OwnerType, ProposalId};
use crate::db::models::agreement::AgreementId;
use crate::db::models::Demand as ModelDemand;
use crate::db::models::Offer as ModelOffer;
use crate::db::schema::{market_negotiation, market_proposal};

/// TODO: Could we avoid having separate enum type for database
///  and separate for client?
#[derive(FromPrimitive, AsExpression, FromSqlRow, PartialEq, Debug, Clone, Copy)]
#[sql_type = "Integer"]
pub enum ProposalState {
    /// Proposal arrived from the market as response to subscription
    Initial = 0,
    /// Bespoke counter-proposal issued by one party directly to other party (negotiation phase)
    Draft = 1,
    /// Rejected by other party
    Rejected = 2,
    /// Promoted to the Agreement draft
    Accepted = 3,
    /// Not accepted nor rejected before validity period
    Expired = 4,
}

/// Represents negotiation between Requestor and Provider related
/// to single Demand/Offer pair. Note that still there can be multiple
/// Negotiation objects related to single Demand/Offer pair, because after
/// terminating Agreement, Requestor and Provider can negotiate new Agreement
/// (but this is not supported yet).
///
/// Note: Some fields in this structure are sometimes redundant, like for example
/// we could deduce requestor and provider NodeId from Offer. But Offers
/// can be removed from our database (after expiration for example)
/// and we still will be able to know, who negotiated with whom.
#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_negotiation"]
pub struct Negotiation {
    pub id: String,
    pub subscription_id: SubscriptionId,
    /// These fields reference initial Offer and Demand for which Proposal was generated.
    /// Note that one of this fields will be equal to subscription_id, depending we are
    /// Provider or Requestor.
    pub offer_id: SubscriptionId,
    pub demand_id: SubscriptionId,

    /// TODO: Use NodeId in all identity_id, requestor_id, provider_id.
    /// Owner of this Negotiation record on local yagna daemon.
    pub identity_id: NodeId,
    /// Ids of negotiating nodes (identities).
    pub requestor_id: NodeId,
    pub provider_id: NodeId,

    /// This field is None, as long Agreement wasn't negotiated (or negotiations
    /// can be broken and never finish with Agreement)
    pub agreement_id: Option<AgreementId>,
}

/// Represent smallest negotiation artifact.
/// Proposal is generated on Requestor side, when matching Offer and Demand is found.
/// Note that initial proposal for Requestor contains properties and constraints
/// from matched Offer. The same applies to initial Proposal for Provider, which contains
/// Demand properties and constraints.
///
/// Proposal id to, be unique, must be generated from Provider and Requestor
/// subscription ids and creation timestamp.
#[derive(Clone, Debug, Identifiable, Insertable, Queryable)]
#[table_name = "market_proposal"]
pub struct DbProposal {
    pub id: ProposalId,
    pub prev_proposal_id: Option<ProposalId>,

    pub negotiation_id: String,

    pub properties: String,
    pub constraints: String,

    pub state: ProposalState,
    pub creation_ts: NaiveDateTime,
    pub expiration_ts: NaiveDateTime,
}

/// Proposal together with Negotiation object related with it.
pub struct Proposal {
    pub negotiation: Negotiation,
    pub body: DbProposal,
}

impl Proposal {
    pub fn new_initial(demand: ModelDemand, offer: ModelOffer) -> Proposal {
        let negotiation = Negotiation::new(&demand, &offer, OwnerType::Requestor);
        let creation_ts = Utc::now().naive_utc();
        // TODO: How to set expiration? Config?
        let expiration_ts = creation_ts + Duration::minutes(10);
        let proposal_id =
            ProposalId::generate_id(&offer.id, &demand.id, &creation_ts, OwnerType::Requestor);

        let proposal = DbProposal {
            id: proposal_id,
            prev_proposal_id: None,
            negotiation_id: negotiation.id.clone(),
            properties: offer.properties,
            constraints: offer.constraints,
            state: ProposalState::Initial,
            creation_ts,
            expiration_ts,
        };

        Proposal {
            body: proposal,
            negotiation,
        }
    }

    pub fn new_provider_initial(demand: ModelDemand, offer: ModelOffer) -> Proposal {
        let negotiation = Negotiation::new(&demand, &offer, OwnerType::Provider);
        let proposal_id = ProposalId::generate_id(
            &offer.id,
            &demand.id,
            &demand.creation_ts,
            OwnerType::Provider,
        );

        let proposal = DbProposal {
            id: proposal_id,
            prev_proposal_id: None,
            negotiation_id: negotiation.id.clone(),
            properties: demand.properties,
            constraints: demand.constraints,
            state: ProposalState::Draft,
            creation_ts: demand.creation_ts,
            expiration_ts: demand.expiration_ts,
        };

        Proposal {
            body: proposal,
            negotiation,
        }
    }

    pub fn from_counter(&self, proposal_id: ProposalId, demand: ModelDemand) -> Proposal {
        let proposal = DbProposal {
            id: proposal_id,
            prev_proposal_id: Some(self.body.id.clone()),
            negotiation_id: self.negotiation.id.clone(),
            properties: demand.properties,
            constraints: demand.constraints,
            state: ProposalState::Draft,
            creation_ts: demand.creation_ts,
            expiration_ts: demand.expiration_ts,
        };

        Proposal {
            body: proposal,
            negotiation: self.negotiation.clone(),
        }
    }

    pub fn counter_with(mut self, proposal: &ClientProposal) -> Proposal {
        let owner = self.body.id.owner();
        let creation_ts = Utc::now().naive_utc();
        // TODO: How to set expiration? Config?
        let expiration_ts = creation_ts + Duration::minutes(10);
        let proposal_id = ProposalId::generate_id(
            &self.negotiation.offer_id,
            &self.negotiation.demand_id,
            &creation_ts,
            owner,
        );

        self.body.prev_proposal_id = Some(self.body.id.clone());
        self.body.id = proposal_id;
        self.body.properties = proposal.properties.to_string();
        self.body.constraints = proposal.constraints.clone();
        self.body.creation_ts = creation_ts;
        self.body.expiration_ts = expiration_ts;
        self.body.state = ProposalState::Draft;
        // We leave negotiation id the same.

        self
    }

    pub fn into_client(self) -> Result<ClientProposal, ErrorMessage> {
        let properties = serde_json::from_str(&self.body.properties).map_err(|error| {
            format!(
                "Can't serialize Proposal properties from database!!! Error: {}",
                error
            )
        })?;

        Ok(ClientProposal {
            properties,
            constraints: self.body.constraints,
            proposal_id: Some(self.body.id.to_string()),
            issuer_id: Some(self.negotiation.provider_id.to_string()),
            state: Some(State::from(self.body.state)),
            prev_proposal_id: self.body.prev_proposal_id.map(|id| id.to_string()),
        })
    }
}

impl Negotiation {
    fn new(demand: &ModelDemand, offer: &ModelOffer, role: OwnerType) -> Negotiation {
        let subscription_id = match role {
            OwnerType::Provider => offer.id.clone(),
            OwnerType::Requestor => demand.id.clone(),
        };

        let identity_id = match role {
            OwnerType::Provider => offer.node_id.clone(),
            OwnerType::Requestor => demand.node_id.clone(),
        };

        Negotiation {
            id: generate_random_id(),
            subscription_id,
            offer_id: offer.id.clone(),
            demand_id: demand.id.clone(),
            identity_id,
            requestor_id: demand.node_id.clone(),
            provider_id: offer.node_id.clone(),
            agreement_id: None,
        }
    }
}

impl From<ProposalState> for State {
    fn from(state: ProposalState) -> Self {
        match state {
            ProposalState::Initial => State::Initial,
            ProposalState::Rejected => State::Rejected,
            ProposalState::Draft => State::Draft,
            ProposalState::Accepted => State::Accepted,
            ProposalState::Expired => State::Expired,
        }
    }
}

impl<DB: Backend> ToSql<Integer, DB> for ProposalState
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<W: std::io::Write>(&self, out: &mut Output<W, DB>) -> diesel::serialize::Result {
        (*self as i32).to_sql(out)
    }
}

impl<DB> FromSql<Integer, DB> for ProposalState
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
