extern crate market_api;
extern crate chrono;

use chrono::*;

use market_api::resolver::properties::*;
use market_api::resolver::errors::ParseError;

#[test]
fn from_type_and_value_str() {
    let prop_value = PropertyValue::from_type_and_value(Some("String"), "some string");
    
    assert_eq!(prop_value, Ok(PropertyValue::Str("some string")));
}

#[test]
fn from_type_and_value_int_ok() {
    let prop_value = PropertyValue::from_type_and_value(Some("Int"), "123");
    
    assert_eq!(prop_value, Ok(PropertyValue::Int(123)));
}

#[test]
fn from_type_and_value_int_error() {
    let prop_value = PropertyValue::from_type_and_value(Some("Int"), "1dblah23");
    
    assert_eq!(prop_value, Err(ParseError::new("Error parsing as Int: '1dblah23'") ));
}

#[test]
fn from_type_and_value_long_ok() {
    let prop_value = PropertyValue::from_type_and_value(Some("Long"), "123");
    
    assert_eq!(prop_value, Ok(PropertyValue::Long(123)));
}

#[test]
fn from_type_and_value_long_error() {
    let prop_value = PropertyValue::from_type_and_value(Some("Long"), "1dblah23");
    
    assert_eq!(prop_value, Err(ParseError::new("Error parsing as Long: '1dblah23'") ));
}

#[test]
fn from_type_and_value_float_ok() {
    let prop_value = PropertyValue::from_type_and_value(Some("Float"), "123.45");
    
    assert_eq!(prop_value, Ok(PropertyValue::Float(123.45)));
}

#[test]
fn from_type_and_value_float_error() {
    let prop_value = PropertyValue::from_type_and_value(Some("Float"), "1dblah23");
    
    assert_eq!(prop_value, Err(ParseError::new("Error parsing as Float: '1dblah23'") ));
}

#[test]
fn from_type_and_value_datetime_ok() {
    let prop_value = PropertyValue::from_type_and_value(Some("DateTime"), "1996-12-19T16:39:57-07:00");

    assert_eq!(prop_value, Ok(PropertyValue::DateTime(Utc.ymd(1996,12,19).and_hms(23,39,57))));
}

#[test]
fn from_type_and_value_datetime_error() {
    let prop_value = PropertyValue::from_type_and_value(Some("DateTime"), "1dblah23");
    
    assert_eq!(prop_value, Err(ParseError::new("Error parsing as DateTime: '1dblah23'") ));
}
