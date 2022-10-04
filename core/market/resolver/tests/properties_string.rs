use ya_market_resolver::resolver::properties::*;

// #region String type

#[test]
fn equals_for_strings_simple_true() {
    let prop_value = PropertyValue::Str("abc");

    assert!(prop_value.equals("abc"));
}

#[test]
fn equals_for_strings_simple_false() {
    let prop_value = PropertyValue::Str("abc");

    assert!(!prop_value.equals("abas"));
}

#[test]
fn equals_for_strings_wildcard_true() {
    let prop_value = PropertyValue::Str("abc");

    assert!(prop_value.equals("ab*"));
}

#[test]
fn equals_for_strings_wildcard_false() {
    let prop_value = PropertyValue::Str("abc");

    assert!(!prop_value.equals("as*"));
}

// #endregion
