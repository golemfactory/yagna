use chrono::{Duration, NaiveDateTime, TimeZone, Utc};

use super::SubscriptionId;

pub enum State {
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
    pub identity_id: String,
    /// Ids of negotiating nodes (identities).
    pub requestor_id: String,
    pub provider_id: String,

    /// This field is None, as long Agreement wasn't negotiated (or negotiations
    /// can be broken and never finish with Agreement)
    pub agreement_id: Option<String>,
}

/// Represent smallest negotiation artifact.
/// Proposal is generated on Requestor side, when matching Offer and Demand is found.
/// Note that initial proposal for Requestor contains properties and constraints
/// from matched Offer. The same applies to initial Proposal for Provider, which contains
/// Demand properties and constraints.
///
/// Proposal id to, be unique, must be generated from properties, constraints
/// Provider and Requestor subscription ids and creation timestamp.
pub struct Proposal {
    pub proposal_id: String,
    pub prev_proposal_id: Option<String>,

    pub negotiation_id: String,

    pub properties: String,
    pub constraints: String,

    pub state: State,
    pub creation_ts: NaiveDateTime,
    pub expiration_ts: NaiveDateTime,
}

