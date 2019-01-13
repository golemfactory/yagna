extern crate uuid;
extern crate chrono;
extern crate regex;

use std::collections::HashMap;
use chrono::{DateTime, Utc};
use regex::Regex;

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
            PropertyValue::Str(value) => PropertyValue::str_equal_with_wildcard(val, *value),  // enhanced string comparison
            PropertyValue::Int(value) => match val.parse::<i32>() { Ok(parsed_value) => parsed_value == *value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Long(value) => match val.parse::<i64>() { Ok(parsed_value) => parsed_value == *value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::Float(value) => match val.parse::<f64>() { Ok(parsed_value) => parsed_value == *value, _ => false }, // ignore parsing error, assume false  
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(val) { Ok(parsed_value) => { parsed_value == *value }, _ => false }, // ignore parsing error, assume false  
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
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(val) { Ok(parsed_value) => { *value < parsed_value }, _ => false }, // ignore parsing error, assume false  
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
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(val) { Ok(parsed_value) => { *value <= parsed_value }, _ => false }, // ignore parsing error, assume false  
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
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(val) { Ok(parsed_value) => { *value > parsed_value }, _ => false }, // ignore parsing error, assume false  
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
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(val) { Ok(parsed_value) => { *value >= parsed_value }, _ => false }, // ignore parsing error, assume false  
            _ => panic!("Not implemented")
        }
    }

    // Implement string equality with * wildcard
    // Note: Only str1 may contain wildcard
    // TODO my be sensible to move the Regex building to the point where property is parsed...
    fn str_equal_with_wildcard(str1 : &str, str2 : &str) -> bool {
        if str1.contains("*") {
            let regex_text = format!("^{}$", str1.replace("*", ".*"));
            match Regex::new(&regex_text) {
                Ok(regex) => regex.is_match(str2),
                Err(_error) => false
            }
        }
        else
        {
            str1 == str2
        }
    }

    fn parse_date(dt_str : &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        PropertyValue::parse_date_from_rfc3339(dt_str)
    } 

    fn parse_date_from_rfc3339(dt_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        match DateTime::parse_from_rfc3339(dt_str) {
            Ok(parsed_value) => {
                let dt = DateTime::<Utc>::from_utc(parsed_value.naive_utc(), Utc); 
                Ok(dt)
            },
            Err(err) => Err(err)
        }
    }

    // Create PropertyValue from (optional) type name and value string.
    pub fn from_type_and_value(type_name : Option<&str>, value : &'a str) -> Result<PropertyValue<'a>, ParseError> {
        match type_name {
            None => Ok(PropertyValue::Str(value)),
            Some(tn) => match tn {
                "String" => Ok(PropertyValue::Str(value)),
                "Int" => match value.parse::<i32>() { 
                    Ok(parsed_val) => Ok(PropertyValue::Int(parsed_val)), 
                    Err(_err) => Err(ParseError::new(&format!("Error parsing as Int: '{}'", value))) },
                "Long" => match value.parse::<i64>() { 
                    Ok(parsed_val) => Ok(PropertyValue::Long(parsed_val)), 
                    Err(_err) => Err(ParseError::new(&format!("Error parsing as Long: '{}'", value))) },
                "Float" => match value.parse::<f64>() { 
                    Ok(parsed_val) => Ok(PropertyValue::Float(parsed_val)), 
                    Err(_err) => Err(ParseError::new(&format!("Error parsing as Float: '{}'", value))) },
                "DateTime" => match PropertyValue::parse_date(value) { 
                    Ok(parsed_val) => Ok(PropertyValue::DateTime(parsed_val)), 
                    Err(_err) => Err(ParseError::new(&format!("Error parsing as DateTime: '{}'", value))) },
                //TODO "Version" => ...,
                // TODO implement List type
                _ => Ok(PropertyValue::Str(value)) // if no type is specified, String is assumed.
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
    fn parse_flat_prop(prop_flat : &'a str) -> Result<(&'a str, Property<'a>), ParseError> {
        // Parse the property string to extract: property name, property type and property value(s)
        println!("parse_flat_prop: {}", prop_flat);

        match prop_parser::parse_prop_def(prop_flat) {
            Ok((name, value)) =>
            {
                match value {
                    Some(val) => 
                    {
                        match prop_parser::parse_prop_ref_with_type(name) {
                            Ok((name, opt_type)) => {
                                println!("From_type_and_value: {:?}, {:?}", opt_type, val);
                                match PropertyValue::from_type_and_value(opt_type, val) {
                                    Ok(prop_value) => Ok((name, Property::Explicit(name, prop_value, HashMap::new()))),
                                    Err(error) => Err(error)
                                }
                            },
                            Err(error) =>
                                Err(ParseError::new(&error))
                        }
                        
                    },
                    None => Ok((name, Property::Implicit(name)))
                }
            },
            Err(error) => Err(ParseError::new(&format!("Parsing error: {}", error)))
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