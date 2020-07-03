use ya_market_resolver::resolver::properties::*;

// #region List type
#[test]
fn equals_for_list_contains_true() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.equals("abc"), true);
    assert_eq!(prop_value.equals("def"), true);
}

#[test]
fn equals_for_list_contains_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.equals("fds"), false);
}

#[test]
fn equals_for_list_list_equals_true() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.equals("[abc,def]"), true);
}

#[test]
fn equals_for_list_list_different_length_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.equals("[abc,def,xyz]"), false);
}

#[test]
fn equals_for_list_list_different_items_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.equals("[abc,xyz]"), false);
}

#[test]
fn greater_for_list_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.greater("abc"), false);
}

#[test]
fn greater_equal_for_list_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.greater_equal("abc"), false);
}

#[test]
fn less_for_list_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.less("abc"), false);
}

#[test]
fn less_equal_for_list_false() {
    let prop_value = PropertyValue::List(vec![
        Box::new(PropertyValue::Str("abc")),
        Box::new(PropertyValue::Str("def")),
    ]);

    assert_eq!(prop_value.less_equal("abc"), false);
}

// #endregion
