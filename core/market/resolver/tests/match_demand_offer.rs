use ya_market_resolver::{match_demand_offer, Match, MatchError};

mod sample;

use sample::{
    POC_DEMAND_CONSTRAINTS, POC_DEMAND_PROPERTIES_JSON, POC_DEMAND_PROPERTIES_JSON_DEEP,
    POC_OFFER_CONSTRAINTS, POC_OFFER_PROPERTIES_JSON, POC_OFFER_PROPERTIES_JSON_DEEP,
};
use ya_market_resolver::flatten::FlattenError;

#[test]
fn match_empty_should_fail() {
    match match_demand_offer("", "", "", "").unwrap_err() {
        MatchError::FlattenError(FlattenError::SerdeJsonError(_)) => (),
        e => panic!("JSON SerDe error expected, but got: {}", e),
    }
}

#[test]
fn match_empty_constraints_should_fail() {
    match match_demand_offer("{}", "", "", "").unwrap_err() {
        MatchError::PrepareError(_) => (),
        e => panic!("Prepare error expected, but got: {}", e),
    }
}

#[test]
fn match_proper_empty_should_match() {
    assert_eq!(
        match_demand_offer("{}", "()", "{}", "()").unwrap(),
        Match::Yes
    )
}

#[test]
fn match_single_prop_single_constr_should_match() {
    assert_eq!(
        match_demand_offer("{\"foo\": \"bar\"}", "()", "{}", "(foo=bar)",).unwrap(),
        Match::Yes
    );
}

#[test]
fn match_cross_single_prop_single_constr_should_match() {
    assert_eq!(
        match_demand_offer(
            "{\"foo\": \"bar\"}",
            "(qux=baz)",
            "{\"qux\": \"baz\"}",
            "(foo=bar)",
        )
        .unwrap(),
        Match::Yes
    );
}

#[test]
fn match_wrong_property_value_should_not_match() {
    let _ = env_logger::builder().try_init();
    assert_eq!(
        match_demand_offer(
            "{\"foo\": \"bar1\"}",
            "(qux=baz)",
            "{\"qux\": \"baz\"}",
            "(foo=bar)",
        )
        .unwrap(),
        Match::No {
            demand_mismatch: vec![],
            offer_mismatch: vec![],
        }
    );
}

#[test]
fn match_wrong_property_key_should_undefined_match() {
    let _ = env_logger::builder().try_init();
    assert_eq!(
        match_demand_offer(
            "{\"foo1\": \"bar\"}",
            "(qux=baz)",
            "{\"qux\": \"baz\"}",
            "(foo=bar)",
        )
        .unwrap(),
        Match::Undefined {
            demand_mismatch: vec!["foo".to_string()],
            offer_mismatch: vec![],
        }
    );
}

#[test]
fn match_poc_offer_demand_samples() {
    let _ = env_logger::builder().try_init();
    assert_eq!(
        match_demand_offer(
            POC_DEMAND_PROPERTIES_JSON,
            POC_DEMAND_CONSTRAINTS,
            POC_OFFER_PROPERTIES_JSON,
            POC_OFFER_CONSTRAINTS,
        )
        .unwrap(),
        Match::Yes
    );
}

#[test]
fn match_poc_offer_demand_samples_deep() {
    let _ = env_logger::builder().try_init();
    assert_eq!(
        match_demand_offer(
            POC_DEMAND_PROPERTIES_JSON_DEEP,
            POC_DEMAND_CONSTRAINTS,
            POC_OFFER_PROPERTIES_JSON_DEEP,
            POC_OFFER_CONSTRAINTS,
        )
        .unwrap(),
        Match::Yes
    );
}
