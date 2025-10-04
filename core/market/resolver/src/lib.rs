#[macro_use]
extern crate nom;

pub mod flatten;
pub mod resolver;

use resolver::error::MatchError as InternalMatchErorr;

use crate::resolver::properties::PropertyRef;
use flatten::{flatten_properties, FlattenError};
use resolver::error::PrepareError;
pub use resolver::matching::{match_weak, MatchResult};
pub use resolver::prepare::{PreparedDemand, PreparedOffer};

#[derive(Debug, PartialEq, Eq)]
pub enum Match {
    Yes,
    No {
        offer_mismatch: Vec<String>,
        demand_mismatch: Vec<String>,
    },
    Undefined {
        offer_mismatch: Vec<String>,
        demand_mismatch: Vec<String>,
    },
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
        MatchResult::False(from_offer, from_demand) => Ok(Match::No {
            offer_mismatch: extract_names(&from_offer),
            demand_mismatch: extract_names(&from_demand),
        }),
        MatchResult::Undefined((from_offer, _), (from_demand, _)) => Ok(Match::Undefined {
            offer_mismatch: extract_names(&from_offer),
            demand_mismatch: extract_names(&from_demand),
        }),
        MatchResult::Err(e) => Err(e.into()),
    }
}

fn extract_names(props_vec: &[&PropertyRef]) -> Vec<String> {
    props_vec
        .iter()
        .map(|prop| match prop {
            PropertyRef::Value(name, _) => name.to_string(),
            PropertyRef::Aspect(name, aspect, _) => format!("{}[{}]", name, aspect),
        })
        .collect()
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
