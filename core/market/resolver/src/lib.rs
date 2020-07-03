#[macro_use]
extern crate nom;

pub mod flatten;
pub mod resolver;

use resolver::errors::MatchError as InternalMatchErorr;

use flatten::{flatten_properties, FlattenError};
use resolver::errors::PrepareError;
pub use resolver::matching::{match_weak, MatchResult};
pub use resolver::prepare::{PreparedDemand, PreparedOffer};

#[derive(Debug, PartialEq)]
pub enum Match {
    Yes,
    No,
    Undefined,
}

#[derive(thiserror::Error, Debug)]
pub enum MatchError {
    #[error("Match error: {0}")]
    InternalError(#[from] InternalMatchErorr),
    #[error("Prepare error: {0}")]
    PrepareError(#[from] PrepareError),
    #[error("Properties preprocessing error: {0}")]
    FlattenError(#[from] FlattenError),
}

pub fn match_demand_offer(
    demand_properties: &str,
    demand_constraints: &str,
    offer_properties: &str,
    offer_constraints: &str,
) -> Result<Match, MatchError> {
    let demand = Demand::from(demand_properties, demand_constraints)?;
    let prep_demand_result = PreparedDemand::from(&demand)?;
    let offer = Offer::from(offer_properties, offer_constraints)?;
    let prep_offer_result = PreparedOffer::from(&offer)?;

    match match_weak(&prep_demand_result, &prep_offer_result)? {
        MatchResult::True => Ok(Match::Yes),
        MatchResult::False(..) => Ok(Match::No),
        MatchResult::Undefined(..) => Ok(Match::Undefined),
        MatchResult::Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Default)]
pub struct Offer {
    // Properties (expressed in flat form, ie. as lines of text)
    pub properties: Vec<String>,

    // Filter expression
    pub constraints: String,
}

impl Offer {
    pub fn from(properties: &str, constraints: &str) -> Result<Self, MatchError> {
        Ok(Offer {
            properties: flatten_properties(properties)?,
            constraints: constraints.to_string(),
        })
    }
}

#[derive(Debug, Default)]
pub struct Demand {
    // Properties (expressed in flat form, ie. as lines of text)
    pub properties: Vec<String>,

    // Filter expression
    pub constraints: String,
}

impl Demand {
    pub fn from(properties: &str, constraints: &str) -> Result<Self, MatchError> {
        Ok(Demand {
            properties: flatten_properties(properties)?,
            constraints: constraints.to_string(),
        })
    }
}
