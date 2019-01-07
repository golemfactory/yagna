extern crate nom;
extern crate asnom;
extern crate market_api;

use std::collections::HashMap;

use market_api::*;
use market_api::resolver::*;
use market_api::resolver::ldap_parser::parse;
use market_api::resolver::expression::*;

#[test]
fn prepare_offer_error_for_empty() {
    let demand = Demand::default();
    match PreparedDemand::from(&demand) {
        Err(_) => {},
        _ => { assert!(false); }
    }
}

#[test]
fn prepare_demand_error_for_empty() {
    let offer = Offer::default();
    match PreparedOffer::from(&offer) {
        Err(_) => {},
        _ => { assert!(false); }
    }
}

#[test]
fn build_expression_present() {
    let f = "(objectClass=*)";

    let expression = Expression::Present(PropertyRef::Value(String::from("objectClass")));
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

fn run_resolve_test(expr : &str, props : &Vec<(&str, &str)>, expect_result : ResolveResult) {
    let expression = build_expression(&parse(expr).unwrap()).unwrap();

    let mut exp_properties = HashMap::new();

    for prop in props {
        exp_properties.insert(String::from(prop.0), String::from(prop.1));
    }

    let imp_props = vec![];
    let property_set = PropertySet::from(&exp_properties, &imp_props);

    assert_eq!(expression.resolve(&property_set), expect_result);
}

#[test]
fn resolve_present() {
    let f = "(objectClass=*)";

    // test positive 

    run_resolve_test(f, &vec![("objectClass", "Babs Jensen")], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec![("cn", "Dblah")], ResolveResult::False);
}

#[test]
fn build_expression_equals() {
    let f = "(cn=Babs Jensen)";

    let expression = Expression::Equals(PropertyRef::Value(String::from("cn")), String::from("Babs Jensen"));
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn resolve_equals() {
    let f = "(cn=Babs Jensen)";

    // test positive

    run_resolve_test(f, &vec![("cn", "Babs Jensen")], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec![("cn", "Dblah")], ResolveResult::False);

    // test undefined

    run_resolve_test(f, &vec![("cnas", "Dblah")], ResolveResult::Undefined);
}


#[test]
fn build_expression_not() {
    let f = "(!(cn=Tim Howes))";

    let expression = Expression::Not( 
            Box::new(Expression::Equals(
                    PropertyRef::Value(String::from("cn")), 
                    String::from("Tim Howes")))
    );
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn resolve_not() {
    let f = "(!(cn=Tim Howes))";

    // test positive

    run_resolve_test(f, &vec![("cn", "Babs Jensen")], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec![("cn", "Tim Howes")], ResolveResult::False);

    // test undefined

    run_resolve_test(f, &vec![("cnas", "Dblah")], ResolveResult::Undefined);
}

#[test]
fn build_expression_and() {
    let f = "(&(a=b)(b=c)(c=d))";

    let expression = Expression::And( 
            vec![
                Box::new(Expression::Equals(
                    PropertyRef::Value(String::from("a")), 
                    String::from("b"))),
                Box::new(Expression::Equals(
                    PropertyRef::Value(String::from("b")), 
                    String::from("c"))),
                Box::new(Expression::Equals(
                    PropertyRef::Value(String::from("c")), 
                    String::from("d"))),
            ]
    );
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn resolve_and() {
    let f = "(&(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec![("a", "b"), ("b", "c"), ("c", "d")], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec![("a", "x"), ("b", "c"), ("c", "d")], ResolveResult::False);

    // test undefined

    run_resolve_test(f, &vec![("b", "c"), ("c", "d")], ResolveResult::Undefined);
}

#[test]
fn resolve_or() {
    let f = "(|(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec![("a", "b"), ("b", "c"), ("c", "d")], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec![("a", "x"), ("b", "y"), ("c", "z")], ResolveResult::False);

    // test undefined

    run_resolve_test(f, &vec![("b", "c"), ("c", "d")], ResolveResult::Undefined);
}

#[test]
fn resolve_complex() {
    let f = "(&(|(a=b)(b=c)(c=d))(!(x=dblah)))";

    // test positive

    run_resolve_test(f, &vec![("a", "b"), ("b", "x"), ("c", "y"), ("x", "notdblah")], ResolveResult::True);
}