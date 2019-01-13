extern crate uuid;
extern crate chrono;

use std::collections::HashMap;
use chrono::{DateTime, Utc};

use super::errors::{ ParseError };
use super::prop_parser;

// #region PropertyValue
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
    
    // TODO Implement equals() for remaining types
    pub fn equals(&self, val : &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value == val,  // trivial string comparison
            PropertyValue::Int(value) => match val.parse::<i32>() { Ok(parsed_value) => parsed_value == *value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Long(value) => match val.parse::<i64>() { Ok(parsed_value) => parsed_value == *value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Float(value) => match val.parse::<f64>() { Ok(parsed_value) => parsed_value == *value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::DateTime(value) => match self.parse_date(val) { Ok(parsed_value) => { parsed_value == *value }, _ => false }, // ignore parsing error, assume false  
            _ => panic!("Not implemented")
        }
    }

    // TODO Implement less() for remaining types
    pub fn less(&self, val : &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value < val,  // trivial string comparison
            PropertyValue::Int(value) => match val.parse::<i32>() { Ok(parsed_value) => *value < parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Long(value) => match val.parse::<i64>() { Ok(parsed_value) => *value < parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Float(value) => match val.parse::<f64>() { Ok(parsed_value) => *value < parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::DateTime(value) => match self.parse_date(val) { Ok(parsed_value) => { *value < parsed_value }, _ => false }, // ignore parsing error, assume false  
            _ => panic!("Not implemented")
        }
    }

    // TODO Implement less_equal() for remaining types
    pub fn less_equal(&self, val : &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value <= val,  // trivial string comparison
            PropertyValue::Int(value) => match val.parse::<i32>() { Ok(parsed_value) => *value <= parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Long(value) => match val.parse::<i64>() { Ok(parsed_value) => *value <= parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Float(value) => match val.parse::<f64>() { Ok(parsed_value) => *value <= parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::DateTime(value) => match self.parse_date(val) { Ok(parsed_value) => { *value <= parsed_value }, _ => false }, // ignore parsing error, assume false  
            _ => panic!("Not implemented")
        }
    }

    // TODO Implement greater() for remaining types
    pub fn greater(&self, val : &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value > val,  // trivial string comparison
            PropertyValue::Int(value) => match val.parse::<i32>() { Ok(parsed_value) => *value > parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Long(value) => match val.parse::<i64>() { Ok(parsed_value) => *value > parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Float(value) => match val.parse::<f64>() { Ok(parsed_value) => *value > parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::DateTime(value) => match self.parse_date(val) { Ok(parsed_value) => { *value > parsed_value }, _ => false }, // ignore parsing error, assume false  
            _ => panic!("Not implemented")
        }
    }

    // TODO Implement greater_equal() for remaining types
    pub fn greater_equal(&self, val : &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value >= val,  // trivial string comparison
            PropertyValue::Int(value) => match val.parse::<i32>() { Ok(parsed_value) => *value >= parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Long(value) => match val.parse::<i64>() { Ok(parsed_value) => *value >= parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Float(value) => match val.parse::<f64>() { Ok(parsed_value) => *value >= parsed_value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::DateTime(value) => match self.parse_date(val) { Ok(parsed_value) => { *value >= parsed_value }, _ => false }, // ignore parsing error, assume false  
            _ => panic!("Not implemented")
        }
    }

    fn parse_date(&self, dt_str : &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        self.parse_date_from_rfc3339(dt_str)
    } 

    fn parse_date_from_rfc3339(&self, dt_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        match DateTime::parse_from_rfc3339(dt_str) {
            Ok(parsed_value) => {
                let dt = DateTime::<Utc>::from_utc(parsed_value.naive_utc(), Utc); 
                Ok(dt)
            },
            Err(err) => Err(err)
        }
    }

    // Create PropertyValue from (optional) type name and value string.
    pub fn from_type_and_value(type_name : Option<&str>, value : &'a str) -> PropertyValue<'a> {
        match type_name {
            None => PropertyValue::Str(value),
            Some(tn) => match tn {
                "String" => PropertyValue::Str(value),
                // TODO implement remaining types
                _ => PropertyValue::Str(value) // if no type is specified, String is assumed.
            }
        }
    }

}

// #endregion

// Property - describes the property with its value and aspects.
#[derive(Debug, Clone, PartialEq)]
pub enum Property<'a> {
    Explicit(&'a str, PropertyValue<'a>, HashMap<&'a str, &'a str>),  // name, values, aspects
    Implicit(&'a str),  // name
}

// #region PropertySet

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
            match PropertySet::parse_flat_prop(prop_flat) {
                Ok((prop_name, prop_value)) => 
                {
                    result.properties.insert(prop_name, prop_value);
                },
                Err(_error) => {
                    // do nothing??? ignore the faulty property
                }
            }
        }

        result
    }

    // Parsing of property values/types
    fn parse_flat_prop(prop_flat : &'a str) -> Result<(&'a str, Property<'a>), String> {
        // Parse the property string to extract: property name, property type and property value(s)
        
        match prop_parser::parse_prop_def(prop_flat) {
            Ok((name, value)) =>
            {
                match value {
                    Some(val) => 
                    {
                        match prop_parser::parse_prop_ref_with_type(name) {
                            Ok((name, opt_type)) => 
                                Ok((name, Property::Explicit(name, PropertyValue::from_type_and_value(opt_type, val), HashMap::new()))),
                            Err(error) =>
                                Err(error)
                        }
                        
                    },
                    None => Ok((name, Property::Implicit(name)))
                }
            },
            Err(error) => Err(format!("Parsing error: {}", error))
        }
    }

    // Set property aspect
    pub fn set_property_aspect(&mut self, prop_name: &'a str, aspect_name: &'a str, aspect_value: &'a str) {
        match self.properties.remove(prop_name) {
            Some(prop) => {
                let mut new_prop = match prop {
                    Property::Explicit(name, val, mut aspects) => {
                            // remove aspect if already exists
                            aspects.remove(aspect_name);
                            aspects.insert(aspect_name, aspect_value);
                            Property::Explicit(name, val, aspects) 
                        } ,
                    _ => unreachable!()
                };
                self.properties.insert(prop_name, new_prop);
            },
            None => {}
        }
    }
}

// #endregion

// Property reference (element of filter expression)
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyRef {
    Value(String), // reference to property value (prop name)
    Aspect(String, String), // reference to property aspect (prop name, aspect name)
}

pub fn parse_prop_ref(flat_prop : &str) -> Result<PropertyRef, ParseError> {
    // TODO parse the flat_prop using prop_parser and repack to PropertyRef
    match prop_parser::parse_prop_ref_with_aspect(flat_prop) {
        Ok((name, opt_aspect)) => {
            match opt_aspect {
                Some(aspect) => Ok(PropertyRef::Aspect(name.to_string(), aspect.to_string())),
                None => Ok(PropertyRef::Value(name.to_string()))
            }
            
        },
        Err(error) => Err(ParseError::new(&format!("Parse error {}", error)))
    }
}