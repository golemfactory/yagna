use chrono::{NaiveDateTime, TimeZone, Utc};
use diesel::sql_types::Text;
use serde::{Deserialize, Serialize};

use ya_client::model::market::proposal::{Proposal as ClientProposal, State};
use ya_client::model::market::NewProposal;
use ya_client::model::{ErrorMessage, NodeId};
use ya_diesel_utils::DbTextField;

use super::{generate_random_id, SubscriptionId};
use super::{Owner, ProposalId};
use crate::db::model::agreement::AgreementId;
use crate::db::model::proposal_id::ProposalIdValidationError;
use crate::db::model::Demand as ModelDemand;
use crate::db::model::Offer as ModelOffer;
use crate::db::schema::{market_negotiation, market_proposal};
use crate::protocol::negotiation::messages::ProposalContent;

/// TODO: Could we avoid having separate enum type for database
///  and separate for client?
#[derive(
    strum_macros::EnumString,
    DbTextField,
    derive_more::Display,
    AsExpression,
    FromSqlRow,
    PartialEq,
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
)]
#[sql_type = "Text"]
pub enum ProposalState {
    /// Proposal arrived from the market as response to subscription
    Initial,
    /// Bespoke counter-proposal issued by one party directly to other party (negotiation phase)
    Draft,
    /// Rejected by other party
    Rejected,
    /// Promoted to the Agreement draft
    Accepted,
    /// Not accepted nor rejected before validity period
    Expired,
}

#[derive(
    DbTextField,
    strum_macros::EnumString,
    derive_more::Display,
    AsExpression,
    FromSqlRow,
    PartialEq,
    Debug,
    Clone,
    Copy,
)]
#[sql_type = "Text"]
pub enum Issuer {
    Us = 0,
    Them = 1,
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

    pub issuer: Issuer,
    pub negotiation_id: String,

    pub properties: String,
    pub constraints: String,

    pub state: ProposalState,
    pub creation_ts: NaiveDateTime,
    pub expiration_ts: NaiveDateTime,
}

/// Proposal together with Negotiation object related with it.
#[derive(Debug)]
pub struct Proposal {
    pub negotiation: Negotiation,
    pub body: DbProposal,
}

impl Proposal {
    pub fn new_requestor(demand: ModelDemand, offer: ModelOffer) -> Proposal {
        let negotiation = Negotiation::from_subscriptions(&demand, &offer, Owner::Requestor);
        let creation_ts = Utc::now().naive_utc();
        let expiration_ts = match demand.expiration_ts < offer.expiration_ts {
            true => demand.expiration_ts.clone(),
            false => offer.expiration_ts.clone(),
        };

        let proposal_id =
            ProposalId::generate_id(&offer.id, &demand.id, &creation_ts, Owner::Requestor);

        let proposal = DbProposal {
            id: proposal_id,
            prev_proposal_id: None,
            issuer: Issuer::Them,
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

    pub fn new_provider(
        demand_id: &SubscriptionId,
        requestor_id: NodeId,
        offer: ModelOffer,
    ) -> Proposal {
        let negotiation = Negotiation::new(
            demand_id,
            requestor_id,
            &offer.id,
            offer.node_id,
            Owner::Provider,
        );

        // TODO: Initial proposal id will differ on Requestor and Provider!!
        let creation_ts = Utc::now().naive_utc();
        let proposal_id =
            ProposalId::generate_id(&offer.id, &demand_id, &creation_ts, Owner::Provider);

        let proposal = DbProposal {
            id: proposal_id,
            prev_proposal_id: None,
            issuer: Issuer::Us, // Requestor market generated this Offer originally, but it's like we are issuer.
            negotiation_id: negotiation.id.clone(),
            properties: offer.properties,
            constraints: offer.constraints,
            state: ProposalState::Initial,
            creation_ts,
            expiration_ts: offer.expiration_ts,
        };

        Proposal {
            body: proposal,
            negotiation,
        }
    }

    pub fn from_draft(&self, proposal: ProposalContent) -> Proposal {
        // TODO: validate demand_proposal.proposal_id with newly generated proposal_id
        let proposal = DbProposal {
            id: proposal.proposal_id,
            issuer: Issuer::Them,
            prev_proposal_id: Some(self.body.id.clone()),
            negotiation_id: self.negotiation.id.clone(),
            properties: proposal.properties,
            constraints: proposal.constraints,
            state: ProposalState::Draft,
            creation_ts: proposal.creation_ts,
            expiration_ts: proposal.expiration_ts,
        };

        Proposal {
            body: proposal,
            negotiation: self.negotiation.clone(),
        }
    }

    pub fn from_client(
        &self,
        proposal: &NewProposal,
        expiration_ts: &NaiveDateTime,
    ) -> Result<Proposal, serde_json::error::Error> {
        let owner = self.body.id.owner();
        let creation_ts = Utc::now().naive_utc();
        let proposal_id = ProposalId::generate_id(
            &self.negotiation.offer_id,
            &self.negotiation.demand_id,
            &creation_ts,
            owner,
        );

        let proposal = DbProposal {
            id: proposal_id,
            prev_proposal_id: Some(self.body.id.clone()),
            issuer: Issuer::Us,
            negotiation_id: self.negotiation.id.clone(),
            properties: serde_json::to_string(&ya_agreement_utils::agreement::flatten(
                proposal.properties.clone(),
            ))?,
            constraints: proposal.constraints.clone(),
            state: ProposalState::Draft,
            creation_ts,
            expiration_ts: expiration_ts.clone(),
        };

        Ok(Proposal {
            body: proposal,
            negotiation: self.negotiation.clone(),
        })
    }

    pub fn into_client(self) -> Result<ClientProposal, ErrorMessage> {
        let properties = serde_json::from_str(&self.body.properties).map_err(|error| {
            format!(
                "Can't serialize Proposal properties from database!!! Error: {}",
                error
            )
        })?;

        let issuer = self.issuer();
        Ok(ClientProposal {
            properties,
            constraints: self.body.constraints,
            proposal_id: self.body.id.to_string(),
            issuer_id: issuer,
            state: State::from(self.body.state),
            timestamp: Utc.from_utc_datetime(&self.body.creation_ts),
            prev_proposal_id: self.body.prev_proposal_id.map(|id| id.to_string()),
        })
    }

    pub fn issuer(&self) -> NodeId {
        match self.body.issuer {
            Issuer::Us => match self.body.id.owner() {
                Owner::Requestor => self.negotiation.requestor_id.clone(),
                Owner::Provider => self.negotiation.provider_id.clone(),
            },
            Issuer::Them => match self.body.id.owner() {
                Owner::Requestor => self.negotiation.provider_id.clone(),
                Owner::Provider => self.negotiation.requestor_id.clone(),
            },
        }
    }

    pub fn validate_id(&self) -> Result<(), ProposalIdValidationError> {
        Ok(self.body.id.validate(
            &self.negotiation.offer_id,
            &self.negotiation.demand_id,
            &self.body.creation_ts,
        )?)
    }
}

impl Negotiation {
    fn from_subscriptions(demand: &ModelDemand, offer: &ModelOffer, role: Owner) -> Negotiation {
        Negotiation::new(&demand.id, demand.node_id, &offer.id, offer.node_id, role)
    }

    fn new(
        demand_id: &SubscriptionId,
        requestor_id: NodeId,
        offer_id: &SubscriptionId,
        provider_id: NodeId,
        role: Owner,
    ) -> Negotiation {
        let subscription_id = match role {
            Owner::Provider => offer_id.clone(),
            Owner::Requestor => demand_id.clone(),
        };

        Negotiation {
            id: generate_random_id(),
            subscription_id,
            offer_id: offer_id.clone(),
            demand_id: demand_id.clone(),
            requestor_id,
            provider_id,
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
