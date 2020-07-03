use ya_market_resolver::resolver::properties::*;

// #region String type

#[test]
fn equals_for_strings_simple_true() {
    let prop_value = PropertyValue::Str("abc");

    assert_eq!(prop_value.equals("abc"), true);
}

#[test]
fn equals_for_strings_simple_false() {
    let prop_value = PropertyValue::Str("abc");

    assert_eq!(prop_value.equals("abas"), false);
}

#[test]
fn equals_for_strings_wildcard_true() {
    let prop_value = PropertyValue::Str("abc");

    assert_eq!(prop_value.equals("ab*"), true);
}

#[test]
fn equals_for_strings_wildcard_false() {
    let prop_value = PropertyValue::Str("abc");

    assert_eq!(prop_value.equals("as*"), false);
}

// #endregion
