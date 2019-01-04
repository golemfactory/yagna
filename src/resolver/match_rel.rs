use super::super::{ Demand, Offer };
use super::ldap_parser::parse;
use super::*;

use std::error;
use std::fmt;
use std::str;

// MatchError

#[derive(Debug, Clone, PartialEq)]
pub struct MatchError {
    msg : String
}

impl MatchError {
    fn new(message : &str) -> Self 
    {
        MatchError{ msg : String::from(message) }
    }
}

impl fmt::Display for MatchError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for MatchError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// Weak match relation
// TODO : "PreparedDemand", "PreparedOffer" structs with preprocessed expressions and properties.ResolveResult
//
pub fn match_weak(demand : &Demand, offer : &Offer) -> Result<ResolveResult, MatchError> {
    let demand_cons_tags = match parse(&demand.constraints) {
        Ok(tags) => tags ,
        Err(error) => { return Err(MatchError::new(&format!("Error parsing Demand constraints: {}", error)))}
    };

    let offer_cons_tags = match parse(&offer.constraints) {
        Ok(tags) => tags ,
        Err(error) => { return Err(MatchError::new(&format!("Error parsing Offer constraints: {}", error)))}
    };

    let demand_cons_expr = match build_expression(&demand_cons_tags) {
        Ok(expr) => expr,
        Err(error) => { return Err(MatchError::new(&format!("Error building Demand constraints expression: {}", error)))}
    };

    let offer_cons_expr = match build_expression(&offer_cons_tags) {
        Ok(expr) => expr,
        Err(error) => { return Err(MatchError::new(&format!("Error building Offer constraints expression: {}", error)))}
    };

    let offer_property_set = PropertySet {
        exp_properties : offer.exp_properties.clone(),
        imp_properties : offer.imp_properties.clone()
    };

    let demand_property_set = PropertySet {
        exp_properties : demand.exp_properties.clone(),
        imp_properties : demand.imp_properties.clone()
    };

    let result1 = demand_cons_expr.resolve(&offer_property_set);
    let result2 = offer_cons_expr.resolve(&demand_property_set);

    match result1 {
        ResolveResult::Err(error) => { return Err(MatchError::new(&format!("Error resolving Demand constraints: {}", error))) },
        _ => {}
    }

    match result2 {
        ResolveResult::Err(error) => { return Err(MatchError::new(&format!("Error resolving Offer constraints: {}", error))) },
        _ => {}
    }

    if result1 == ResolveResult::Undefined || result2 == ResolveResult::Undefined {
        Ok(ResolveResult::Undefined)
    }
    else { 
        if result1 == ResolveResult::True || result2 == ResolveResult::True {
            Ok(ResolveResult::True)
        } 
        else 
        {
            Ok(ResolveResult::False)
        }
    }
}

