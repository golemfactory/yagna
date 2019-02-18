extern crate nom;
extern crate asnom;
extern crate market_api;

use market_api::*;
use market_api::resolver::*;
use market_api::resolver::properties::*;
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
fn build_expression_empty() {
    let f = "()";

    let expression = Expression::Empty;
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn build_expression_present() {
    let f = "(objectClass=*)";

    let expression = Expression::Present(PropertyRef::Value(String::from("objectClass")));
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

fn run_resolve_test(expr : &str, props : &Vec<&str>, expect_result : ResolveResult) {
    let expression = build_expression(&parse(expr).unwrap()).unwrap();

    let mut properties = vec![];
    for prop in props {
        properties.push(prop.to_string());
    }

    let property_set = PropertySet::from_flat_props(&properties);

    assert_eq!(expression.resolve(&property_set), expect_result);
}

#[test]
fn resolve_empty() {
    let f = "()";

    // test positive 

    run_resolve_test(f, &vec!["objectClass=\"Babs Jensen\""], ResolveResult::True);
}

#[test]
fn resolve_present() {
    let f = "(objectClass=*)";

    // test positive 

    run_resolve_test(f, &vec!["objectClass=\"Babs Jensen\""], ResolveResult::True);

    // test negative (must return name of unresolved property)

    run_resolve_test(f, &vec!["cn=\"Dblah\""], ResolveResult::False(vec![&PropertyRef::Value(String::from("objectClass"))]));
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

    run_resolve_test(f, &vec!["cn=\"Babs Jensen\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=\"Dblah\""], ResolveResult::False(vec![]));

    // test undefined

    run_resolve_test(f, &vec!["cnas=\"Dblah\""], ResolveResult::Undefined(vec![&PropertyRef::Value(String::from("cn", ))]));
}

#[test]
fn resolve_equals_with_wildcard() {
    let f = "(cn=Babs *)";

    // test positive

    run_resolve_test(f, &vec!["cn=\"Babs Jensen\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=\"Dblah\""], ResolveResult::False(vec![]));

    // test undefined

    run_resolve_test(f, &vec!["cnas=\"Dblah\""], ResolveResult::Undefined(vec![&PropertyRef::Value(String::from("cn"))]));
}

#[test]
fn resolve_equals_int() {
    let f = "(cn=123)";

    // test positive

    run_resolve_test(f, &vec!["cn=123"], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=456"], ResolveResult::False(vec![]));

    // test false when parsing error

    let f = "(cn=1ds23)";
    run_resolve_test(f, &vec!["cn=123"], ResolveResult::False(vec![]));

}

#[test]
fn resolve_greater_int() {
    let f = "(cn>123)";

    // test positive

    run_resolve_test(f, &vec!["cn=124"], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=12"], ResolveResult::False(vec![]));

    // test false when parsing error

    let f = "(cn>1ds23)";
    run_resolve_test(f, &vec!["cn=123"], ResolveResult::False(vec![]));

}

#[test]
fn resolve_less_float() {
    let f = "(cn<123.56)";

    // test positive

    run_resolve_test(f, &vec!["cn=122.674"], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=126"], ResolveResult::False(vec![]));

    // test false when parsing error

    let f = "(cn<1ds23)";
    run_resolve_test(f, &vec!["cn=123"], ResolveResult::False(vec![]));

}

#[test]
fn resolve_less_equal_datetime() {
    let f = "(cn<=1985-04-12T23:20:50.52Z)";

    // test positive

    run_resolve_test(f, &vec!["cn=t\"1985-04-12T23:20:30.52Z\""], ResolveResult::True);
    run_resolve_test(f, &vec!["cn=t\"1985-04-12T23:20:50.52Z\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=t\"1985-04-12T23:21:50.52Z\""], ResolveResult::False(vec![]));

    // test false when parsing error (NOTE the RFC 3339 format is fairly strict)

    let f = "(cn<=1985-04-13)";
    run_resolve_test(f, &vec!["cn=t\"1985-04-12T23:20:50.52Z\""], ResolveResult::False(vec![]));

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

    run_resolve_test(f, &vec!["cn=\"Babs Jensen\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=\"Tim Howes\""], ResolveResult::False(vec![]));

    // test undefined

    run_resolve_test(f, &vec!["cnas=\"Dblah\""], ResolveResult::Undefined(vec![&PropertyRef::Value(String::from("cn"))]));
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

    run_resolve_test(f, &vec!["a=\"b\"", "b=\"c\"", "c=\"d\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["a=\"x\"", "b=\"c\"", "c=\"d\""], ResolveResult::False(vec![]));

    // test undefined

    run_resolve_test(f, &vec!["b=\"c\"", "c=\"d\""], ResolveResult::Undefined(vec![&PropertyRef::Value(String::from("a"))]));
}

#[test]
fn resolve_or() {
    let f = "(|(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec!["a=\"b\"", "b=\"c\"", "c=\"d\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["a=\"x\"", "b=\"y\"", "c=\"z\""], ResolveResult::False(vec!()));

    // test undefined

    run_resolve_test(f, &vec!["b=\"c\"", "c=\"d\""], ResolveResult::Undefined(vec![&PropertyRef::Value(String::from("a"))]));
}

#[test]
fn resolve_complex() {
    let f = "(&(|(a=b)(b=c)(c=d))(!(x=dblah)))";

    // test positive

    run_resolve_test(f, &vec!["a=\"b\"", "b=\"x\"", "c=\"y\"", "x=\"notdblah\""], ResolveResult::True);
}