extern crate uuid;
extern crate chrono;

pub mod errors;
pub mod ldap_parser;
pub mod prop_parser;
pub mod matching;
pub mod expression;

use std::collections::HashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};

use self::errors::{ PrepareError };
use super::{ NodeId, Demand, Offer };
use self::expression::build_expression;

pub use self::matching::match_weak;
pub use self::expression::Expression;

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue<'a> {
    Str(&'a str), // String 
    Int(i32), 
    Long(i64),
    Float(f64),
    DateTime(DateTime<Utc>),
    Version(&'a str),
    List(Vec<PropertyValue<'a>>),
}

impl <'a> PropertyValue<'a> {
    // TODO Implement equals() for all types
    // for now trivial string implementation
    pub fn equals(&self, val : &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value == val,  // trivial string comparison
            _ => panic!("Not implemented")
        }
    }
}

// Property - describes the property with its value and aspects.
#[derive(Debug, Clone, PartialEq)]
pub enum Property<'a> {
    Explicit(&'a str, PropertyValue<'a>, HashMap<&'a str, &'a str>),  // name, values, aspects
    Implicit(&'a str),  // name
}


#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropertySet <'a>{
    pub properties : HashMap<&'a str, Property<'a>>,
}

impl <'a> PropertySet<'a> {
    // Create PropertySet from vector of properties expressed in flat form (ie. by parsing)
    pub fn from_flat_props(props : &'a Vec<String>) -> PropertySet<'a> {
        let mut result = PropertySet{
            properties : HashMap::new()
        };

        // parse and pack props
        for prop_flat in props {
            let (prop_name, prop_value) = PropertySet::parse_flat_prop(prop_flat);
            result.properties.insert(prop_name, prop_value);
        }

        result
    }

    // Create PropertySet from hashmap of explicit props and vector of implicit props
    pub fn from(exp_props : &'a HashMap<String, String>, imp_props : &'a Vec<String>) -> PropertySet<'a> {
        let mut result = PropertySet{
            properties : HashMap::new()
        };

        // re-pack explicit props
        for key in exp_props.keys() {
            let (prop_name, prop_value) = PropertySet::parse_prop(key, &exp_props.get(key).unwrap());
            let prop = Property::Explicit(prop_name, prop_value, HashMap::new());
            result.properties.insert(prop_name, prop);
        }

        // re-pack implicit props
        for key in imp_props {
            let prop = Property::Implicit(&key);
            result.properties.insert(&key, prop);
        }

        result
    }

    // TODO Remove this after flat prop parsing implemented
    // for now - trivial string implementation
    fn parse_prop(key : &'a str, value_string : &'a str) -> (&'a str, PropertyValue<'a>) {
        (key, PropertyValue::Str(value_string))
    }

    // TODO Implement parsing of property values/types here
    // for now - dummy implementation
    fn parse_flat_prop(prop_flat : &'a str) -> (&'a str, Property<'a>) {
        // TODO parse the property string to extract: property name, property type and property value(s)
        
        ("key", Property::Implicit("key"))
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
