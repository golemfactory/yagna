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
fn regex_metacharacters_are_literal() {
    let prop_value = PropertyValue::Str("axb");
    assert!(!prop_value.equals("a.b"));

    let prop_value = PropertyValue::Str("a.b");
    assert!(prop_value.equals("a.b"));
    assert!(prop_value.equals("a*b"));
}

#[test]
fn equals_for_strings_wildcard_true() {
    let prop_value = PropertyValue::Str("abc");

    assert!(prop_value.equals("ab*"));
    assert!(prop_value.equals("*"));
    assert!(prop_value.equals("a**c"));
    assert!(prop_value.equals("*b*"));

    let empty = PropertyValue::Str("");
    assert!(empty.equals("*"));
}

#[test]
fn equals_for_strings_wildcard_false() {
    let prop_value = PropertyValue::Str("abc");

    assert!(!prop_value.equals("as*"));
}

// #endregion
