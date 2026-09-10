use super::super::{Demand, Offer};
use super::constraint_parser;
use super::error::PrepareError;
use super::expression::Expression;
use super::properties::PropertySet;

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedOffer<'a> {
    pub properties: PropertySet<'a>,
    pub constraints: Expression,
}

impl<'a> PreparedOffer<'a> {
    pub fn from(offer: &'a Offer) -> Result<Self, PrepareError> {
        let (properties, constraints) = prepare(&offer.properties, &offer.constraints, "Offer")?;
        Ok(Self {
            properties,
            constraints,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedDemand<'a> {
    pub properties: PropertySet<'a>,
    pub constraints: Expression,
}

impl<'a> PreparedDemand<'a> {
    pub fn from(demand: &'a Demand) -> Result<Self, PrepareError> {
        let (properties, constraints) = prepare(&demand.properties, &demand.constraints, "Demand")?;
        Ok(Self {
            properties,
            constraints,
        })
    }
}

fn prepare<'a>(
    properties: &'a [String],
    constraints: &str,
    side: &str,
) -> Result<(PropertySet<'a>, Expression), PrepareError> {
    let constraints = constraint_parser::parse(constraints)
        .map_err(|error| PrepareError::new(format!("Error parsing {side} constraints: {error}")))?;
    let properties = PropertySet::from_flat_props(properties)
        .map_err(|error| PrepareError::new(format!("Error parsing {side} properties: {error}")))?;
    Ok((properties, constraints))
}
