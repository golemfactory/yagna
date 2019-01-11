use super::prepare::{PreparedDemand, PreparedOffer};
use super::expression::{ResolveResult};
use super::errors::{MatchError};

// Matching relation result enum
#[derive(Debug, Clone, PartialEq)]
pub enum MatchResult {
    True,
    False,
    Undefined,
    Err(MatchError)
}

// Weak match relation
//
pub fn match_weak(demand : &PreparedDemand, offer : &PreparedOffer) -> Result<MatchResult, MatchError> {

    println!("Demand: {:?}", demand);
    println!("Offer: {:?}", offer);


    let result1 = demand.constraints.resolve(&offer.properties);
    let result2 = offer.constraints.resolve(&demand.properties);

    match result1 {
        ResolveResult::Err(error) => { return Err(MatchError::new(&format!("Error resolving Demand constraints: {}", error))) },
        _ => {}
    }

    match result2 {
        ResolveResult::Err(error) => { return Err(MatchError::new(&format!("Error resolving Offer constraints: {}", error))) },
        _ => {}
    }

    println!("Result1: {:?}", result1);
    println!("Result2: {:?}", result2);

    if result1 == ResolveResult::Undefined || result2 == ResolveResult::Undefined {
        Ok(MatchResult::Undefined)
    }
    else { 
        if result1 == ResolveResult::True && result2 == ResolveResult::True {
            Ok(MatchResult::True)
        } 
        else 
        {
            Ok(MatchResult::False)
        }
    }
}

