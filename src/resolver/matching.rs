use super::prepare::{PreparedDemand, PreparedOffer};
use super::expression::{ResolveResult};
use super::errors::{MatchError};

// Matching relation result enum
#[derive(Debug, Clone, PartialEq)]
pub enum MatchResult {
    True,
    False(Vec<String>, Vec<String>), // Unresolved properties in Offer and Demand respectively
    Undefined(Vec<String>, Vec<String>), // Unresolved properties in Offer and Demand respectively
    Err(MatchError)
}

// Weak match relation
//
pub fn match_weak<'a>(demand : &'a PreparedDemand, offer : &'a PreparedOffer) -> Result<MatchResult, MatchError> {

    println!("Demand: {:?}", demand);
    println!("Offer: {:?}", offer);


    let result1 = demand.constraints.resolve(&offer.properties);
    let result2 = offer.constraints.resolve(&demand.properties);

    println!("Result1: {:?}", result1);
    println!("Result2: {:?}", result2);

    let mut un_props1 = vec![]; // undefined properties in result 1
    let mut un_props2 = vec![]; // undefined properties in result 2

    let mut result1_binary = false;
    let mut result2_binary = false;
    let mut result1_undefined = false;
    let mut result2_undefined = false;

    match result1 {
        ResolveResult::True => { result1_binary = true; },
        ResolveResult::False(mut un_props) => {  result1_binary = false; un_props1.append(&mut un_props); },
        ResolveResult::Undefined(mut un_props) => { result1_undefined = true; un_props1.append(&mut un_props); },
        ResolveResult::Err(error) => { return Err(MatchError::new(&format!("Error resolving Demand constraints: {}", error))); },
        _ => {}
    };

    match result2 {
        ResolveResult::True => { result2_binary = true; },
        ResolveResult::False(mut un_props) => {  result2_binary = false; un_props2.append(&mut un_props); },
        ResolveResult::Undefined(mut un_props) => { result2_undefined = true; un_props2.append(&mut un_props); },
        ResolveResult::Err(error) => { return Err(MatchError::new(&format!("Error resolving Offer constraints: {}", error))); },
        _ => {}
    };


    if result1_undefined || result2_undefined {
        Ok(MatchResult::Undefined(un_props1, un_props2))
    }
    else { 
        if result1_binary == true && result2_binary == true {
            Ok(MatchResult::True)
        } 
        else 
        {
            Ok(MatchResult::False(un_props1, un_props2))
        }
    }
}

