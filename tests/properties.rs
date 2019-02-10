extern crate market_api;
extern crate chrono;

use std::collections::*;

use chrono::*;

use market_api::resolver::properties::*;
use market_api::resolver::errors::ParseError;

#[test]
fn from_value_str() {
    let prop_value = PropertyValue::from_value("\"some string\"");
    
    assert_eq!(prop_value, Ok(PropertyValue::Str("some string")));
}

#[test]
fn from_value_bool_true_ok() {
    let prop_value = PropertyValue::from_value("true");
    
    assert_eq!(prop_value, Ok(PropertyValue::Boolean(true)));
}

#[test]
fn from_value_bool_false_ok() {
    let prop_value = PropertyValue::from_value("false");
    
    assert_eq!(prop_value, Ok(PropertyValue::Boolean(false)));
}

#[test]
fn from_value_number_ok() {
    let prop_value = PropertyValue::from_value("123");
    
    assert_eq!(prop_value, Ok(PropertyValue::Number(123.0)));
}

#[test]
fn from_value_number_error() {
    let prop_value = PropertyValue::from_value("1dblah23");
    
    assert_eq!(prop_value, Err(ParseError::new("Error parsing literal: '1dblah23'") ));
}

#[test]
fn from_value_number_float_ok() {
    let prop_value = PropertyValue::from_value("123.45");
    
    assert_eq!(prop_value, Ok(PropertyValue::Number(123.45)));
}

#[test]
fn from_value_datetime_ok() {
    let prop_value = PropertyValue::from_value("t\"1996-12-19T16:39:57-07:00\"");

    assert_eq!(prop_value, Ok(PropertyValue::DateTime(Utc.ymd(1996,12,19).and_hms(23,39,57))));
}

#[test]
fn from_value_datetime_error() {
    let prop_value = PropertyValue::from_value("t\"1dblah23\"");
    
    assert_eq!(prop_value, Err(ParseError::new("Error parsing as DateTime: '1dblah23'") ));
}

#[test]
fn from_value_literal_error() {
    let prop_value = PropertyValue::from_value("Babs Jensen"); // No quotes
    
    assert_eq!(prop_value, Err(ParseError::new("Error parsing literal: 'Babs Jensen'") ));
}

#[test]
fn from_flat_props_ok() {

    let props = vec![String::from("objectClass=Babs Jensen")];
    
    let property_set = PropertySet::from_flat_props(&props);

    assert_eq!(property_set, PropertySet{ 
        properties : HashMap::new()
    });
}
