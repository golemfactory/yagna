#[macro_use]
extern crate nom;

pub mod resolver;

// use resolver::errors::MatchError;
//
// pub use resolver::matching::{match_weak, MatchResult};
// pub use resolver::prepare::{PreparedDemand, PreparedOffer};

// pub fn match_demand_offer(
//     demand_properties: String,
//     demand_constraints: String,
//     offer_properties: String,
//     offer_constraints: String,
// ) -> Result<MatchResult<'a>, MatchError> {
//     let prep_demand_result = PreparedDemand::from(demand_properties, demand_constraints)?;
//     let prep_offer_result = PreparedOffer::from(offer_properties, offer_constraints)?;
//
//     match_weak(&prep_demand_result, &prep_offer_result)
// }

#[derive(Debug, Default)]
pub struct Offer {
    // Properties (expressed in flat form, ie. as lines of text)
    pub properties: Vec<String>,

    // Filter expression
    pub constraints: String,
}

#[derive(Debug, Default)]
pub struct Demand {
    // Properties (expressed in flat form, ie. as lines of text)
    pub properties: Vec<String>,

    // Filter expression
    pub constraints: String,
}
