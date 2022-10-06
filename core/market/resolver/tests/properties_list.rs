use ya_market_resolver::resolver::properties::*;

// #region List type
#[test]
fn equals_for_list_contains_true() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(prop_value.equals("abc"));
    assert!(prop_value.equals("def"));
}

#[test]
fn equals_for_list_contains_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(!prop_value.equals("fds"));
}

#[test]
fn equals_for_list_list_equals_true() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(prop_value.equals("[abc,def]"));
}

#[test]
fn equals_for_list_list_different_length_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(!prop_value.equals("[abc,def,xyz]"));
}

#[test]
fn equals_for_list_list_different_items_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(!prop_value.equals("[abc,xyz]"));
}

#[test]
fn greater_for_list_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(!prop_value.greater("abc"));
}

#[test]
fn greater_equal_for_list_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(!prop_value.greater_equal("abc"));
}

#[test]
fn less_for_list_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(!prop_value.less("abc"));
}

#[test]
fn less_equal_for_list_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert!(!prop_value.less_equal("abc"));
}

// #endregion
