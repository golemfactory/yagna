use std::collections::*;

use chrono::*;

use ya_market_resolver::resolver::error::ParseError;
use ya_market_resolver::resolver::properties::*;

// #region from_value()
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

    assert_eq!(
        prop_value,
        Err(ParseError::new("Error parsing literal: '1dblah23'"))
    );
}

#[test]
fn from_value_number_float_ok() {
    let prop_value = PropertyValue::from_value("123.45");

    assert_eq!(prop_value, Ok(PropertyValue::Number(123.45)));
}

#[test]
fn from_value_decimal_ok() {
    let prop_value = PropertyValue::from_value("d\"123\"");

    assert_eq!(
        prop_value,
        Ok(PropertyValue::Decimal("123.0".parse().unwrap()))
    );
}

#[test]
fn from_value_decimal_long_ok() {
    let prop_value =
        PropertyValue::from_value("d\"123456789123456789123456789.123456789123456789\"");

    assert_eq!(
        prop_value,
        Ok(PropertyValue::Decimal(
            "123456789123456789123456789.123456789123456789"
                .parse()
                .unwrap()
        ))
    );
}

#[test]
fn from_value_decimal_error() {
    let prop_value = PropertyValue::from_value("d\"123");

    assert_eq!(
        prop_value,
        Err(ParseError::new("Error parsing literal: 'd\"123'"))
    );
}

#[test]
fn from_value_decimal_error2() {
    let prop_value = PropertyValue::from_value("d\"12sasd3\"");

    assert_eq!(
        prop_value,
        Err(ParseError::new("Error parsing as Decimal: '12sasd3'"))
    );
}

#[test]
fn from_value_datetime_ok() {
    let prop_value = PropertyValue::from_value("t\"1996-12-19T16:39:57-07:00\"");

    assert_eq!(
        prop_value,
        Ok(PropertyValue::DateTime(
            Utc.with_ymd_and_hms(1996, 12, 19, 23, 39, 57).unwrap()
        ))
    );
}

#[test]
fn from_value_datetime_error() {
    let prop_value = PropertyValue::from_value("t\"1dblah23\"");

    assert_eq!(
        prop_value,
        Err(ParseError::new("Error parsing as DateTime: '1dblah23'"))
    );
}

#[test]
fn from_value_version_ok() {
    let prop_value = PropertyValue::from_value("v\"1.3.0\"");

    assert_eq!(
        prop_value,
        Ok(PropertyValue::Version(
            semver::Version::parse("1.3.0").unwrap()
        ))
    );
}

#[test]
fn from_value_literal_error() {
    let prop_value = PropertyValue::from_value("Babs Jensen"); // No quotes

    assert_eq!(
        prop_value,
        Err(ParseError::new("Error parsing literal: 'Babs Jensen'"))
    );
}

#[test]
fn from_value_list_ok() {
    let prop_value = PropertyValue::from_value("[\"abc\",\"def\"]");

    assert_eq!(
        prop_value,
        Ok(PropertyValue::List(vec![
            Box::new(PropertyValue::Str("abc")),
            Box::new(PropertyValue::Str("def"))
        ]))
    );
}

#[test]
fn from_value_list_error() {
    let prop_value = PropertyValue::from_value("[\"abc\",asdasdas]");

    assert_eq!(
        prop_value,
        Err(ParseError::new(
            "Error parsing literal: '[\"abc\",asdasdas]'"
        ))
    );
}

#[test]
fn from_flat_props_ok() {
    let props = vec![String::from("objectClass=\"Babs Jensen\"")];

    let property_set = PropertySet::from_flat_props(&props);

    assert_eq!(
        property_set,
        PropertySet {
            properties: {
                let mut x = HashMap::new();
                x.insert(
                    "objectClass",
                    Property::Explicit(
                        "objectClass",
                        PropertyValue::Str("Babs Jensen"),
                        HashMap::new(),
                    ),
                );
                x
            }
        }
    );
}

// #endregion
