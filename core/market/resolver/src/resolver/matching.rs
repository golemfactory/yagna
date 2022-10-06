use super::error::MatchError;
use super::expression::{Expression, ResolveResult};
use super::prepare::{PreparedDemand, PreparedOffer};
use super::properties::PropertyRef;

// Matching relation result enum
#[derive(Debug, Clone, PartialEq)]
pub enum MatchResult<'a> {
    True,
    False(Vec<&'a PropertyRef>, Vec<&'a PropertyRef>), // Unresolved properties in Offer and Demand respectively
    Undefined(
        (Vec<&'a PropertyRef>, Expression),
        (Vec<&'a PropertyRef>, Expression),
    ), // Unresolved properties, unreduced expression - in Offer and Demand respectively
    Err(MatchError),
}

// Weak match relation
//
pub fn match_weak<'a>(
    demand: &'a PreparedDemand,
    offer: &'a PreparedOffer,
) -> Result<MatchResult<'a>, MatchError> {
    log::trace!("Demand: {:?}", demand);
    log::trace!("Offer: {:?}", offer);

    let result1 = demand.constraints.resolve(&offer.properties);
    let result2 = offer.constraints.resolve(&demand.properties);

    log::trace!("Demand constraints with Offer properties: {:?}", result1);
    log::trace!("Offer constraints with Demand properties: {:?}", result2);

    let mut un_props1 = vec![]; // undefined properties in result 1
    let mut un_props2 = vec![]; // undefined properties in result 2

    let mut result1_binary = false;
    let mut result2_binary = false;
    let mut result1_undefined = false;
    let mut result2_undefined = false;

    let mut result1_unres_expr = Expression::Empty(true);
    let mut result2_unres_expr = Expression::Empty(true);

    match result1 {
        ResolveResult::True => {
            result1_binary = true;
        }
        ResolveResult::False(mut un_props, unresolved_expr) => {
            result1_binary = false;
            un_props1.append(&mut un_props);
            result1_unres_expr = unresolved_expr;
        }
        ResolveResult::Undefined(mut un_props, unresolved_expr) => {
            result1_undefined = true;
            un_props1.append(&mut un_props);
            result1_unres_expr = unresolved_expr;
        }
        ResolveResult::Err(error) => {
            return Err(MatchError::new(&format!(
                "Error resolving Demand constraints: {}",
                error
            )));
        }
    };

    match result2 {
        ResolveResult::True => {
            result2_binary = true;
        }
        ResolveResult::False(mut un_props, unresolved_expr) => {
            result2_binary = false;
            un_props2.append(&mut un_props);
            result2_unres_expr = unresolved_expr;
        }
        ResolveResult::Undefined(mut un_props, unresolved_expr) => {
            result2_undefined = true;
            un_props2.append(&mut un_props);
            result2_unres_expr = unresolved_expr;
        }
        ResolveResult::Err(error) => {
            return Err(MatchError::new(&format!(
                "Error resolving Offer constraints: {}",
                error
            )));
        }
    };

    if result1_undefined || result2_undefined {
        Ok(MatchResult::Undefined(
            (un_props1, result1_unres_expr),
            (un_props2, result2_unres_expr),
        ))
    } else if result1_binary == true && result2_binary == true {
        Ok(MatchResult::True)
    } else {
        Ok(MatchResult::False(un_props1, un_props2))
    }
}
