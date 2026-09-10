use std::str;

use bigdecimal::BigDecimal;
use chrono::{DateTime, TimeZone, Utc};
use semver::Version;
use std::collections::{HashMap, VecDeque};

use super::error::ParseError;
use super::prop_parser;
use super::prop_parser::Literal;

#[allow(non_camel_case_types)]
type d128 = BigDecimal;

// #region PropertyValue
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue<'a> {
    Str(&'a str), // Str
    Boolean(bool),
    //Int(i32),
    //Long(i64),
    Number(f64),
    Decimal(BigDecimal),
    DateTime(DateTime<Utc>),
    Version(Version),
    List(Vec<PropertyValue<'a>>),
}

impl<'a> PropertyValue<'a> {
    // TODO Implement equals() for remaining types
    pub fn equals(&self, other: &str) -> bool {
        match self {
            PropertyValue::Str(value) => PropertyValue::str_equal_with_wildcard(other, value), // enhanced string comparison
            PropertyValue::Number(value) => match other.parse::<f64>() {
                Ok(parsed_value) => parsed_value == *value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Decimal(value) => match other.parse::<BigDecimal>() {
                Ok(parsed_value) => parsed_value == *value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(other) {
                Ok(parsed_value) => parsed_value == *value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Version(value) => match Version::parse(other) {
                Ok(parsed_value) => parsed_value == *value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::List(value) => PropertyValue::equals_list(value, other),
            PropertyValue::Boolean(value) => match other.parse::<bool>() {
                Ok(result) => &result == value,
                _ => false,
            }, // ignore parsing error, assume false
        }
    }

    // TODO Implement less() for remaining types
    pub fn less(&self, other: &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value < other, // trivial string comparison
            PropertyValue::Number(value) => match other.parse::<f64>() {
                Ok(parsed_value) => *value < parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Decimal(value) => match other.parse::<d128>() {
                Ok(parsed_value) => *value < parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(other) {
                Ok(parsed_value) => *value < parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Version(value) => match Version::parse(other) {
                Ok(parsed_value) => *value < parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::List(_) => false,             // operator meaningless for List
            PropertyValue::Boolean(_) => false,          // operator meaningless for bool
        }
    }

    // TODO Implement less_equal() for remaining types
    pub fn less_equal(&self, other: &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value <= other, // trivial string comparison
            PropertyValue::Number(value) => match other.parse::<f64>() {
                Ok(parsed_value) => *value <= parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Decimal(value) => match other.parse::<d128>() {
                Ok(parsed_value) => *value <= parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(other) {
                Ok(parsed_value) => *value <= parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Version(value) => match Version::parse(other) {
                Ok(parsed_value) => *value <= parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::List(_) => false,              // operator meaningless for List
            PropertyValue::Boolean(_) => false,           // operator meaningless for bool
        }
    }

    // TODO Implement greater() for remaining types
    pub fn greater(&self, other: &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value > other, // trivial string comparison
            PropertyValue::Number(value) => match other.parse::<f64>() {
                Ok(parsed_value) => *value > parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Decimal(value) => match other.parse::<d128>() {
                Ok(parsed_value) => *value > parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(other) {
                Ok(parsed_value) => *value > parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Version(value) => match Version::parse(other) {
                Ok(parsed_value) => *value > parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::List(_) => false,             // operator meaningless for List
            PropertyValue::Boolean(_) => false,          // operator meaningless for bool
        }
    }

    // TODO Implement greater_equal() for remaining types
    pub fn greater_equal(&self, other: &str) -> bool {
        match self {
            PropertyValue::Str(value) => *value >= other, // trivial string comparison
            PropertyValue::Number(value) => match other.parse::<f64>() {
                Ok(parsed_value) => *value >= parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Decimal(value) => match other.parse::<d128>() {
                Ok(parsed_value) => *value >= parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::DateTime(value) => match PropertyValue::parse_date(other) {
                Ok(parsed_value) => *value >= parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::Version(value) => match Version::parse(other) {
                Ok(parsed_value) => *value >= parsed_value,
                _ => false,
            }, // ignore parsing error, assume false
            PropertyValue::List(_) => false,              // operator meaningless for List
            PropertyValue::Boolean(_) => false,           // operator meaningless for bool
        }
    }

    // Implement string equality with * as the only wildcard metacharacter.
    // Note: Only str1 may contain a wildcard.
    fn str_equal_with_wildcard(str1: &str, str2: &str) -> bool {
        let pattern = str1.as_bytes();
        let value = str2.as_bytes();
        let (mut pattern_at, mut value_at) = (0, 0);
        let mut last_star = None;
        let mut retry_value_at = 0;

        while value_at < value.len() {
            if pattern.get(pattern_at) == value.get(value_at) {
                pattern_at += 1;
                value_at += 1;
            } else if pattern.get(pattern_at) == Some(&b'*') {
                last_star = Some(pattern_at);
                pattern_at += 1;
                retry_value_at = value_at;
            } else if let Some(star) = last_star {
                retry_value_at += 1;
                value_at = retry_value_at;
                pattern_at = star + 1;
            } else {
                return false;
            }
        }

        while pattern.get(pattern_at) == Some(&b'*') {
            pattern_at += 1;
        }
        pattern_at == pattern.len()
    }

    fn parse_date(dt_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        PropertyValue::parse_date_from_rfc3339(dt_str)
    }

    fn parse_date_from_rfc3339(dt_str: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
        match DateTime::parse_from_rfc3339(dt_str) {
            Ok(parsed_value) => {
                let dt = Utc.from_utc_datetime(&parsed_value.naive_utc());
                Ok(dt)
            }
            Err(err) => Err(err),
        }
    }

    // Create PropertyValue from value string.
    pub fn from_value(value: &'a str) -> Result<PropertyValue<'a>, ParseError> {
        match prop_parser::parse_prop_value_literal(value) {
            Ok(tag) => PropertyValue::from_literal(tag),
            Err(_error) => Err(ParseError::new(format!(
                "Error parsing literal: '{}'",
                value
            ))),
        }
    }

    // Convert self into different type of property value
    // Returns:
    // - If conversion possible or not needed:
    //   None - if conversion not required
    //   Some(new PropertyValue) - if conversion required
    // - If conversion not possible
    //   Err
    pub fn to_prop_ref_type(
        &self,
        impl_type: &PropertyRefType,
    ) -> Result<Option<PropertyValue>, String> {
        match impl_type {
            PropertyRefType::Any => Ok(None),
            PropertyRefType::Decimal => match self {
                PropertyValue::Decimal(_) => Ok(None),
                PropertyValue::Str(val) => match PropertyValue::from_literal(Literal::Decimal(val))
                {
                    Ok(prop_val) => Ok(Some(prop_val)),
                    Err(error) => Err(format!("{:?}", error)),
                },
                _ => Err(format!("Unable to convert {:?} to {:?}", self, impl_type)),
            },
            PropertyRefType::DateTime => match self {
                PropertyValue::DateTime(_) => Ok(None),
                PropertyValue::Str(val) => {
                    match PropertyValue::from_literal(Literal::DateTime(val)) {
                        Ok(prop_val) => Ok(Some(prop_val)),
                        Err(error) => Err(format!("{:?}", error)),
                    }
                }
                _ => Err(format!("Unable to convert {:?} to {:?}", self, impl_type)),
            },
            PropertyRefType::Version => match self {
                PropertyValue::Version(_) => Ok(None),
                PropertyValue::Str(val) => match PropertyValue::from_literal(Literal::Version(val))
                {
                    Ok(prop_val) => Ok(Some(prop_val)),
                    Err(error) => Err(format!("{:?}", error)),
                },
                _ => Err(format!("Unable to convert {:?} to {:?}", self, impl_type)),
            },
        }
    }

    fn from_literal(literal: Literal<'a>) -> Result<PropertyValue<'a>, ParseError> {
        match literal {
            Literal::Str(val) => Ok(PropertyValue::Str(val)),
            Literal::Number(val) => match val.parse::<f64>() {
                Ok(parsed_val) => Ok(PropertyValue::Number(parsed_val)),
                Err(_err) => Err(ParseError::new(format!(
                    "Error parsing as Number: '{}'",
                    val
                ))),
            },
            Literal::Decimal(val) => match val.parse::<d128>() {
                Ok(parsed_val) => Ok(PropertyValue::Decimal(parsed_val)),
                Err(_err) => Err(ParseError::new(format!(
                    "Error parsing as Decimal: '{}'",
                    val
                ))),
            },
            Literal::DateTime(val) => match PropertyValue::parse_date(val) {
                Ok(parsed_val) => Ok(PropertyValue::DateTime(parsed_val)),
                Err(_err) => Err(ParseError::new(format!(
                    "Error parsing as DateTime: '{}'",
                    val
                ))),
            },
            Literal::Bool(val) => Ok(PropertyValue::Boolean(val)),
            Literal::Version(val) => match Version::parse(val) {
                Ok(parsed_val) => Ok(PropertyValue::Version(parsed_val)),
                Err(_err) => Err(ParseError::new(format!(
                    "Error parsing as Version: '{}'",
                    val
                ))),
            },
            Literal::List(vals) => {
                let results: Result<Vec<PropertyValue<'a>>, ParseError> =
                    vals.into_iter().map(PropertyValue::from_literal).collect();
                results
                    .map(PropertyValue::List)
                    .map_err(|error| ParseError::new(format!("Error parsing list: '{error}'")))
            }
        }
    }

    fn equals_list(list_items: &[PropertyValue], other: &str) -> bool {
        // if val is a proper list syntax - parse it and test list equality
        // otherwise, if val isnt a list - treat it as a single item and execute "IN" operator
        match prop_parser::parse_prop_ref_as_list(other) {
            Ok(list_vals) => {
                if list_vals.len() != list_items.len() {
                    return false;
                }

                PropertyValue::has_perfect_list_matching(list_items, &list_vals)
            }
            Err(_) => {
                for item in list_items {
                    if item.equals(other) {
                        return true;
                    }
                }
                false
            }
        }
    }

    fn has_perfect_list_matching(list_items: &[PropertyValue], constraints: &[&str]) -> bool {
        let item_count = list_items.len();
        let mut constraint_to_item = vec![None; constraints.len()];
        let mut item_to_constraint = vec![None; item_count];
        let mut visited_constraints = vec![0_usize; constraints.len()];
        let mut visited_items = vec![0_usize; item_count];
        let mut item_predecessor = vec![0_usize; item_count];
        let mut queue = VecDeque::with_capacity(constraints.len());

        // Find and reverse augmenting paths iteratively. Unlike a greedy
        // assignment, this can reconsider a broad wildcard when a later,
        // narrower constraint needs the item it initially consumed.
        for first_constraint in 0..constraints.len() {
            let visit = first_constraint + 1;
            visited_constraints[first_constraint] = visit;
            queue.push_back(first_constraint);
            let mut unmatched_item = None;

            while let Some(constraint) = queue.pop_front() {
                for (item, value) in list_items.iter().enumerate() {
                    if visited_items[item] == visit || !value.equals(constraints[constraint]) {
                        continue;
                    }

                    visited_items[item] = visit;
                    item_predecessor[item] = constraint;
                    match item_to_constraint[item] {
                        Some(next_constraint) => {
                            if visited_constraints[next_constraint] != visit {
                                visited_constraints[next_constraint] = visit;
                                queue.push_back(next_constraint);
                            }
                        }
                        None => {
                            unmatched_item = Some(item);
                            break;
                        }
                    }
                }

                if unmatched_item.is_some() {
                    break;
                }
            }

            let Some(mut item) = unmatched_item else {
                return false;
            };
            loop {
                let constraint = item_predecessor[item];
                let previous_item = constraint_to_item[constraint].replace(item);
                item_to_constraint[item] = Some(constraint);
                match previous_item {
                    Some(previous_item) => item = previous_item,
                    None => break,
                }
            }
            queue.clear();
        }

        true
    }
}

// #endregion

// Property - describes the property with its value and aspects.
#[derive(Debug, Clone, PartialEq)]
pub enum Property<'a> {
    Explicit(&'a str, PropertyValue<'a>, HashMap<&'a str, &'a str>), // name, values, aspects
    Implicit(&'a str),                                               // name
}

// #region PropertySet

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PropertySet<'a> {
    pub properties: HashMap<&'a str, Property<'a>>,
}

impl<'a> PropertySet<'a> {
    // Create PropertySet from vector of properties expressed in flat form (ie. by parsing)
    pub fn from_flat_props(props: &'a [String]) -> Result<PropertySet<'a>, ParseError> {
        let mut result = PropertySet {
            properties: HashMap::new(),
        };

        for (index, prop_flat) in props.iter().enumerate() {
            let (property_name, property) =
                PropertySet::parse_flat_prop(prop_flat).map_err(|error| {
                    ParseError::new(format!("Invalid property at index {index}: {error}"))
                })?;
            result.properties.insert(property_name, property);
        }

        Ok(result)
    }

    // Parsing of property values/types
    fn parse_flat_prop(prop_flat: &'a str) -> Result<(&'a str, Property<'a>), ParseError> {
        // Parse the property string to extract: property name and property value(s) - also detecting the property types
        match prop_parser::parse_prop_def(prop_flat) {
            Ok((name, value)) => match value {
                Some(val) => match PropertyValue::from_value(val) {
                    Ok(prop_value) => {
                        Ok((name, Property::Explicit(name, prop_value, HashMap::new())))
                    }
                    Err(error) => Err(error),
                },
                None => Ok((name, Property::Implicit(name))),
            },
            Err(error) => Err(ParseError::new(format!("Parsing error: {}", error))),
        }
    }

    // Set property aspect
    pub fn set_property_aspect(
        &mut self,
        prop_name: &'a str,
        aspect_name: &'a str,
        aspect_value: &'a str,
    ) {
        if let Some(prop) = self.properties.remove(prop_name) {
            let new_prop = match prop {
                Property::Explicit(name, val, mut aspects) => {
                    // remove aspect if already exists
                    aspects.remove(aspect_name);
                    aspects.insert(aspect_name, aspect_value);
                    Property::Explicit(name, val, aspects)
                }
                _ => unreachable!(),
            };
            self.properties.insert(prop_name, new_prop);
        }
    }
}

// Property reference (element of filter expression)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyRef {
    Value(String, PropertyRefType), // reference to property value (prop name)
    Aspect(String, String, PropertyRefType), // reference to property aspect (prop name, aspect name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyRefType {
    Any,
    Decimal,
    Version,
    DateTime,
}

pub fn parse_prop_ref(flat_prop: &str) -> Result<PropertyRef, ParseError> {
    match prop_parser::parse_prop_ref_with_aspect(flat_prop) {
        Ok((name, opt_aspect, impl_type)) => match opt_aspect {
            Some(aspect) => Ok(PropertyRef::Aspect(
                name.to_string(),
                aspect.to_string(),
                decode_implied_ref_type(impl_type),
            )),
            None => Ok(PropertyRef::Value(
                name.to_string(),
                decode_implied_ref_type(impl_type),
            )),
        },
        Err(error) => Err(ParseError::new(format!("Parse error {}", error))),
    }
}

fn decode_implied_ref_type(impl_type: Option<&str>) -> PropertyRefType {
    match impl_type {
        Some("d") => PropertyRefType::Decimal,
        Some("v") => PropertyRefType::Version,
        Some("t") => PropertyRefType::DateTime,
        None => PropertyRefType::Any,
        e => panic!("Unknown implied type code!, got: {:?}", e),
    }
}
