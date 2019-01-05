extern crate uuid;

pub mod errors;
pub mod ldap_parser;
pub mod matching;
pub mod expression;

use std::collections::HashMap;
use uuid::Uuid;

use self::errors::{ PrepareError };
use super::{ NodeId, Demand, Offer };
use self::expression::build_expression;

pub use self::matching::match_weak;
pub use self::expression::Expression;

// Property - describes the property with its value and aspects.
#[derive(Debug, Clone, PartialEq)]
pub enum Property<'a> {
    Explicit(&'a str, &'a str, HashMap<&'a str, &'a str>),  // name, value, aspects
    Implicit(&'a str),  // name
}


#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropertySet <'a>{
    pub properties : HashMap<&'a str, Property<'a>>,
}

impl <'a> PropertySet<'a> {
    // Create PropertySet from hashmap of explicit props and vector of implicit props
    pub fn from(exp_props : &'a HashMap<String, String>, imp_props : &'a Vec<String>) -> PropertySet<'a> {
        let mut result = PropertySet{
            properties : HashMap::new()
        };

        // re-pack explicit props
        for key in exp_props.keys() {
            let prop = Property::Explicit(&key, &exp_props.get(key).unwrap(), HashMap::new());
            result.properties.insert(&key, prop);
        }

        // re-pack implicit props
        for key in imp_props {
            let prop = Property::Implicit(&key);
            result.properties.insert(&key, prop);
        }

        result
    }
}

// PreparedOffer
// Offer parsed and converted into optimized data structures.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedOffer<'a> {
    pub offer_id : Uuid,
    pub provider_id : NodeId,

    // Properties (values, aspects)
    pub properties : PropertySet<'a>,

    // Filter expression
    pub constraints : Expression,
}

impl <'a> PreparedOffer<'a> {
    pub fn from(offer : &'a Offer) -> Result<PreparedOffer, PrepareError> {
        let offer_cons_tags = match ldap_parser::parse(&offer.constraints) {
            Ok(tags) => tags ,
            Err(error) => { return Err(PrepareError::new(&format!("Error parsing Offer constraints: {}", error)))}
        };
        
        let offer_cons_expr = match build_expression(&offer_cons_tags) {
            Ok(expr) => expr,
            Err(error) => { return Err(PrepareError::new(&format!("Error building Offer constraints expression: {}", error)))}
        };

        let result = PreparedOffer{
            offer_id : offer.offer_id.clone(),
            provider_id : offer.provider_id.clone(),
            properties : PropertySet::from(&offer.exp_properties, &offer.imp_properties),
            constraints : offer_cons_expr
        };

        Ok(result)
    }
}

// PreparedDemand
// Offer parsed and converted into optimized data structures.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedDemand<'a> {
    pub demand_id : Uuid,
    pub requestor_id : NodeId,

    // Properties (values, aspects)
    pub properties : PropertySet<'a>,

    // Filter expression
    pub constraints : Expression,
}

impl <'a> PreparedDemand<'a> {
    // Process a Demand to obtain a PreparedDemand
    pub fn from(demand : &'a Demand) -> Result<PreparedDemand, PrepareError> {
        let demand_cons_tags = match ldap_parser::parse(&demand.constraints) {
            Ok(tags) => tags ,
            Err(error) => { return Err(PrepareError::new(&format!("Error parsing Demand constraints: {}", error)))}
        };
        
        let demand_cons_expr = match build_expression(&demand_cons_tags) {
            Ok(expr) => expr,
            Err(error) => { return Err(PrepareError::new(&format!("Error building Demand constraints expression: {}", error)))}
        };

        let result = PreparedDemand{
            demand_id : demand.demand_id.clone(),
            requestor_id : demand.requestor_id.clone(),
            properties : PropertySet::from(&demand.exp_properties, &demand.imp_properties),
            constraints : demand_cons_expr
        };

        Ok(result)
    }
}
