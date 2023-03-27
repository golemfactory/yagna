use ya_market_resolver::flatten::flatten_properties;

mod sample;

use sample::{
    POC_DEMAND_PROPERTIES_FLAT, POC_DEMAND_PROPERTIES_JSON, POC_DEMAND_PROPERTIES_JSON_DEEP,
    POC_OFFER_PROPERTIES_FLAT, POC_OFFER_PROPERTIES_JSON, POC_OFFER_PROPERTIES_JSON_DEEP,
};

#[test]
fn flatten_empty() {
    assert!(flatten_properties("").is_err());
}

#[test]
fn flatten_key_digit() {
    assert_eq_without_order!(flatten_properties(r#"{"key":1}"#).unwrap(), vec!("key=1"));
}

#[test]
fn flatten_2_flat_keys() {
    assert_eq_without_order!(
        flatten_properties(r#"{"key1":1,"key2":2}"#).unwrap(),
        vec!("key1=1", "key2=2")
    );
}

#[test]
fn flatten_2_nested_keys() {
    assert_eq_without_order!(
        flatten_properties(r#"{"n":{"key1":1,"key2":2}}"#).unwrap(),
        vec!("n.key1=1", "n.key2=2")
    );
}

#[test]
fn flatten_2_mixed_keys() {
    assert_eq_without_order!(
        flatten_properties(r#"{"n":{"key1":true},"key2":"two"}"#).unwrap(),
        vec!("key2=\"two\"", "n.key1=true")
    );
}

#[test]
fn flatten_sample_poc_offer_properties() {
    assert_eq_without_order!(
        flatten_properties(POC_OFFER_PROPERTIES_JSON).unwrap(),
        POC_OFFER_PROPERTIES_FLAT
    );
}

#[test]
fn flatten_sample_poc_offer_properties_deep() {
    assert_eq_without_order!(
        flatten_properties(POC_OFFER_PROPERTIES_JSON_DEEP).unwrap(),
        POC_OFFER_PROPERTIES_FLAT
    );
}

#[test]
fn flatten_sample_poc_demand_properties() {
    assert_eq_without_order!(
        flatten_properties(POC_DEMAND_PROPERTIES_JSON).unwrap(),
        POC_DEMAND_PROPERTIES_FLAT
    );
}

#[test]
fn flatten_sample_poc_demand_properties_deep() {
    assert_eq_without_order!(
        flatten_properties(POC_DEMAND_PROPERTIES_JSON_DEEP).unwrap(),
        POC_DEMAND_PROPERTIES_FLAT
    );
}

#[macro_export]
macro_rules! assert_eq_without_order {
    ($left:expr, $right:expr ) => {
        assert_eq!($left.to_vec().sort(), $right.to_vec().sort(), "");
    };
}
