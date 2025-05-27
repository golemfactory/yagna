//! This module is to be used only in tests.
#![allow(dead_code)]
#![allow(unused_macros)]

pub use super::config::*;
pub use super::db::dao::*;
pub use super::db::model::*;
pub use super::db::schema::*;
pub use super::db::{DbError, DbResult};
pub use super::identity::{IdentityApi, IdentityError};
pub use super::matcher::{error::*, *};
pub use super::negotiation::{error::*, ApprovalStatus};
pub use super::protocol::*;

pub mod events_helper;
pub mod market_ext;

// Re-export commonly used types
pub use crate::db::model::{
    Agreement, AgreementEventType, AgreementId, AgreementState, DbProposal, Demand, EventType,
    Issuer, Negotiation, Offer, Owner, Proposal, ProposalId, ProposalState, SubscriptionId,
};
pub use crate::MarketService;

// Re-export types needed by test framework
pub use crate::matcher::EventsListeners;
pub use crate::negotiation::ScannerSet;
pub use crate::protocol::callback::*;
pub use crate::protocol::discovery::{builder::DiscoveryBuilder, error::*, message::*, Discovery};
pub use crate::protocol::negotiation::messages::*;

// Re-export types needed for testing
pub use crate::config::GolemBaseNetwork;
pub use crate::matcher::store::SubscriptionStore;
pub use crate::matcher::Matcher;

// Re-export schema types
pub use crate::db::schema::market_agreement::dsl as agreement_dsl;
pub use crate::db::schema::market_demand::dsl as demand_dsl;
pub use crate::db::schema::market_negotiation::dsl as negotiation_dsl;
pub use crate::db::schema::market_negotiation_event::dsl as event_dsl;
pub use crate::db::schema::market_offer::dsl as offer_dsl;
pub use crate::db::schema::market_proposal::dsl as proposal_dsl;

// Re-export config types
pub use crate::config::{Config, DiscoveryConfig};
pub use crate::db::dao::ProposalDao;
pub use crate::db::DbMixedExecutor;

pub use crate::matcher::error::{DemandError, QueryOfferError};
pub use crate::negotiation::{ApprovalResult, ProviderBroker, RequestorBroker};
pub use crate::protocol::discovery::message::{
    OffersBcast, OffersRetrieved, QueryOffers, QueryOffersResult, RetrieveOffers,
    UnsubscribedOffersBcast,
};

pub use market_ext::MarketServiceExt;
