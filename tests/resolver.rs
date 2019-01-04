extern crate nom;
extern crate asnom;
extern crate market_api;

use market_api::resolver::ldap_parser::parse;
use market_api::resolver::*;

use std::collections::HashMap;
use std::default::Default;
use asnom::common::TagClass;
use asnom::structures::{Tag, OctetString, Sequence, ExplicitTag};

#[test]
fn build_expression_present() {
    let f = "(objectClass=*)";

    let expression = Expression::Present(String::from("objectClass"));
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn resolve_present() {
    let f = "(objectClass=*)";

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    // test positive

    let mut property_set1 = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set1.exp_properties.insert(String::from("objectClass"), String::from("Babs Jensen"));

    assert_eq!(expression.resolve(&property_set1), ResolveResult::True);

    // test negative

    let mut property_set2 = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set2.exp_properties.insert(String::from("cn"), String::from("Dblah"));

    assert_eq!(expression.resolve(&property_set2), ResolveResult::False);

}

#[test]
fn build_expression_equals() {
    let f = "(cn=Babs Jensen)";

    let expression = Expression::Equals(String::from("cn"), String::from("Babs Jensen"));
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn resolve_equals() {
    let f = "(cn=Babs Jensen)";

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    // test positive

    let mut property_set1 = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set1.exp_properties.insert(String::from("cn"), String::from("Babs Jensen"));

    assert_eq!(expression.resolve(&property_set1), ResolveResult::True);

    // test negative

    let mut property_set2 = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set2.exp_properties.insert(String::from("cn"), String::from("Dblah"));

    assert_eq!(expression.resolve(&property_set2), ResolveResult::False);

    // test undefined

    let mut property_set3 = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set3.exp_properties.insert(String::from("cnas"), String::from("Dblah"));

    assert_eq!(expression.resolve(&property_set3), ResolveResult::Undefined);
}


#[test]
fn build_expression_not() {
    let f = "(!(cn=Tim Howes))";

    let expression = Expression::Not( 
            Box::new(Expression::Equals(
                    String::from("cn"), 
                    String::from("Tim Howes")))
    );
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn resolve_not() {
    let f = "(!(cn=Tim Howes))";

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    // test positive

    let mut property_set1 = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set1.exp_properties.insert(String::from("cn"), String::from("Babs Jensen"));

    assert_eq!(expression.resolve(&property_set1), ResolveResult::True);

    // test negative

    let mut property_set2 = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set2.exp_properties.insert(String::from("cn"), String::from("Tim Howes"));

    assert_eq!(expression.resolve(&property_set2), ResolveResult::False);

    // test undefined

    let mut property_set3 = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set3.exp_properties.insert(String::from("cnas"), String::from("Dblah"));

    assert_eq!(expression.resolve(&property_set3), ResolveResult::Undefined);
}

#[test]
fn build_expression_and() {
    let f = "(&(a=b)(b=c)(c=d))";

    let expression = Expression::And( 
            vec![
                Box::new(Expression::Equals(
                    String::from("a"), 
                    String::from("b"))),
                Box::new(Expression::Equals(
                    String::from("b"), 
                    String::from("c"))),
                Box::new(Expression::Equals(
                    String::from("c"), 
                    String::from("d"))),
            ]
    );
    
    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn resolve_and() {
    let f = "(&(a=b)(b=c)(c=d))";

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    // test positive

    let mut property_set = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set.exp_properties.insert(String::from("a"), String::from("b"));
    property_set.exp_properties.insert(String::from("b"), String::from("c"));
    property_set.exp_properties.insert(String::from("c"), String::from("d"));

    assert_eq!(expression.resolve(&property_set), ResolveResult::True);

    // test negative

    let mut property_set = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set.exp_properties.insert(String::from("a"), String::from("x")); // does not match
    property_set.exp_properties.insert(String::from("b"), String::from("c"));
    property_set.exp_properties.insert(String::from("c"), String::from("d"));

    assert_eq!(expression.resolve(&property_set), ResolveResult::False);

    // test undefined

    let mut property_set = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set.exp_properties.insert(String::from("b"), String::from("c"));
    property_set.exp_properties.insert(String::from("c"), String::from("d"));

    assert_eq!(expression.resolve(&property_set), ResolveResult::Undefined);

}

#[test]
fn resolve_or() {
    let f = "(|(a=b)(b=c)(c=d))";

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    // test positive

    let mut property_set = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set.exp_properties.insert(String::from("a"), String::from("b"));
    property_set.exp_properties.insert(String::from("b"), String::from("c"));
    property_set.exp_properties.insert(String::from("c"), String::from("d"));

    assert_eq!(expression.resolve(&property_set), ResolveResult::True);

    // test negative

    let mut property_set = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set.exp_properties.insert(String::from("a"), String::from("x"));
    property_set.exp_properties.insert(String::from("b"), String::from("y"));
    property_set.exp_properties.insert(String::from("c"), String::from("z"));

    assert_eq!(expression.resolve(&property_set), ResolveResult::False);

    // test undefined

    let mut property_set = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set.exp_properties.insert(String::from("b"), String::from("c"));
    property_set.exp_properties.insert(String::from("c"), String::from("d"));

    assert_eq!(expression.resolve(&property_set), ResolveResult::Undefined);

}

#[test]
fn resolve_complex() {
    let f = "(&(|(a=b)(b=c)(c=d))(!(x=dblah)))";

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    // test positive

    let mut property_set = PropertySet{
        exp_properties : HashMap::new(),
        imp_properties : String::default(),
    };

    property_set.exp_properties.insert(String::from("a"), String::from("b"));
    property_set.exp_properties.insert(String::from("b"), String::from("x"));
    property_set.exp_properties.insert(String::from("c"), String::from("y"));
    property_set.exp_properties.insert(String::from("x"), String::from("notdblah"));

    assert_eq!(expression.resolve(&property_set), ResolveResult::True);
}