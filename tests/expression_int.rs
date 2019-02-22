extern crate nom;
extern crate asnom;
extern crate market_api;

use market_api::*;
use market_api::resolver::*;
use market_api::resolver::properties::*;
use market_api::resolver::ldap_parser::parse;
use market_api::resolver::expression::*;

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

    run_resolve_test(f, &vec!["cn=\"Dblah\""], 
                    ResolveResult::False(
                        vec![&PropertyRef::Value(String::from("objectClass"))],
                        Expression::Empty
                    ));
}

#[test]
fn resolve_equals() {
    let f = "(cn=Babs Jensen)";

    // test positive

    run_resolve_test(f, &vec!["cn=\"Babs Jensen\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=\"Dblah\""], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test undefined

    run_resolve_test(f, &vec!["cnas=\"Dblah\""], 
                    ResolveResult::Undefined(
                        vec![&PropertyRef::Value(String::from("cn", ))],
                        Expression::Equals(PropertyRef::Value(String::from("cn")), String::from("Babs Jensen"))
                    ));
}

#[test]
fn resolve_equals_with_wildcard() {
    let f = "(cn=Babs *)";

    // test positive

    run_resolve_test(f, &vec!["cn=\"Babs Jensen\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=\"Dblah\""], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test undefined

    run_resolve_test(f, &vec!["cnas=\"Dblah\""], 
                    ResolveResult::Undefined(
                        vec![&PropertyRef::Value(String::from("cn"))],
                        Expression::Equals(PropertyRef::Value(String::from("cn")), String::from("Babs *"))
                    ));
}

#[test]
fn resolve_equals_int() {
    let f = "(cn=123)";

    // test positive

    run_resolve_test(f, &vec!["cn=123"], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=456"], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test false when parsing error

    let f = "(cn=1ds23)";
    run_resolve_test(f, &vec!["cn=123"], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

}

#[test]
fn resolve_greater_int() {
    let f = "(cn>123)";

    // test positive

    run_resolve_test(f, &vec!["cn=124"], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=12"], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test false when parsing error

    let f = "(cn>1ds23)";
    run_resolve_test(f, &vec!["cn=123"], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

}

#[test]
fn resolve_less_float() {
    let f = "(cn<123.56)";

    // test positive

    run_resolve_test(f, &vec!["cn=122.674"], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=126"], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test false when parsing error

    let f = "(cn<1ds23)";
    run_resolve_test(f, &vec!["cn=123"], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

}

#[test]
fn resolve_less_equal_datetime() {
    let f = "(cn<=1985-04-12T23:20:50.52Z)";

    // test positive

    run_resolve_test(f, &vec!["cn=t\"1985-04-12T23:20:30.52Z\""], ResolveResult::True);
    run_resolve_test(f, &vec!["cn=t\"1985-04-12T23:20:50.52Z\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=t\"1985-04-12T23:21:50.52Z\""], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test false when parsing error (NOTE the RFC 3339 format is fairly strict)

    let f = "(cn<=1985-04-13)";
    run_resolve_test(f, &vec!["cn=t\"1985-04-12T23:20:50.52Z\""], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

}

#[test]
fn resolve_not() {
    let f = "(!(cn=Tim Howes))";

    // test positive

    run_resolve_test(f, &vec!["cn=\"Babs Jensen\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["cn=\"Tim Howes\""], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test undefined

    run_resolve_test(f, &vec!["cnas=\"Dblah\""], 
                    ResolveResult::Undefined(
                        vec![&PropertyRef::Value(String::from("cn"))],
                        Expression::Not(Box::new(Expression::Equals(PropertyRef::Value(String::from("cn")), String::from("Tim Howes"))))
                    ));
}

#[test]
fn resolve_and() {
    let f = "(&(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec!["a=\"b\"", "b=\"c\"", "c=\"d\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["a=\"x\"", "b=\"c\"", "c=\"d\""], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test undefined

    run_resolve_test(f, &vec!["b=\"c\"", "c=\"d\""], 
                    ResolveResult::Undefined(
                        vec![&PropertyRef::Value(String::from("a"))],
                        Expression::And(vec![Box::new(Expression::Equals(PropertyRef::Value(String::from("a")), String::from("b")))])
                    ));
}



#[test]
fn resolve_or() {
    let f = "(|(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec!["a=\"b\"", "b=\"c\"", "c=\"d\""], ResolveResult::True);

    // test negative

    run_resolve_test(f, &vec!["a=\"x\"", "b=\"y\"", "c=\"z\""], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));

    // test undefined

    run_resolve_test(f, &vec!["b=\"x\"", "c=\"y\""], 
                    ResolveResult::Undefined(
                        vec![&PropertyRef::Value(String::from("a"))],
                        Expression::Or(vec![Box::new(Expression::Equals(PropertyRef::Value(String::from("a")), String::from("b")))])
                    ));
}

#[test]
fn resolve_complex() {
    let f = "(&(|(a=b)(b=c)(c=d))(!(x=dblah)))";

    // test positive

    run_resolve_test(f, &vec!["a=\"b\"", "b=\"x\"", "c=\"y\"", "x=\"notdblah\""], ResolveResult::True);
}

#[test]
fn resolve_complex_or_undefined() {
    let f = "(|(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec![/*"a=\"b\"",*/ "b=\"x\"", "c=\"y\"", "x=\"notdblah\""], 
                    ResolveResult::Undefined(
                        vec![&PropertyRef::Value(String::from("a"))],
                        Expression::Or(vec![Box::new(Expression::Equals(PropertyRef::Value(String::from("a")), String::from("b")))])
                    ));
}


#[test]
fn resolve_complex_or_undefined_reduced() {
    let f = "(|(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec![/*"a=\"b\"",*/ "b=\"c\"", "c=\"y\"", "x=\"notdblah\""], 
                    ResolveResult::True);
}

#[test]
fn resolve_complex_and_undefined() {
    let f = "(&(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec![/*"a=\"b\"",*/ "b=\"c\"", "c=\"d\"", "x=\"notdblah\""], 
                    ResolveResult::Undefined(
                        vec![&PropertyRef::Value(String::from("a"))],
                        Expression::And(vec![Box::new(Expression::Equals(PropertyRef::Value(String::from("a")), String::from("b")))])
                    ));
}

#[test]
fn resolve_complex_and_undefined_reduced() {
    let f = "(&(a=b)(b=c)(c=d))";

    // test positive

    run_resolve_test(f, &vec![/*"a=\"b\"",*/ "b=\"c\"", "c=\"x\"", "x=\"notdblah\""], 
                    ResolveResult::False(
                        vec![],
                        Expression::Empty
                    ));
}



#[test]
fn resolve_multistep_with_supplemented_props() {
    let f = "(&(|(a=b)(b=c)(c=d))(!(x=dblah)))";

    let props_partial = vec![/*"a=\"b\"",*/ "b=\"x\"", "c=\"y\"", "x=\"notdblah\""];
    let props_full = vec!["a=\"b\"", "b=\"x\"", "c=\"y\"", "x=\"notdblah\""];

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    // assemble incomplete properties
    let mut properties_partial = vec![];
    for prop in props_partial {
        properties_partial.push(prop.to_string());
    }

    let property_set_partial = PropertySet::from_flat_props(&properties_partial);

    // assemble full properties
    let mut properties_full = vec![];
    for prop in props_full {
        properties_full.push(prop.to_string());
    }

    let property_set_full = PropertySet::from_flat_props(&properties_full);


    // run resolve for incomplete props - expect unresolved property refs and a reduced expression

    let resolve_result_partial = expression.resolve(&property_set_partial);

    assert_eq!(resolve_result_partial, 
            ResolveResult::Undefined(
                vec![&PropertyRef::Value(String::from("a"))],
                Expression::And(vec![
                    Box::new(
                        Expression::Or(vec![Box::new(Expression::Equals(PropertyRef::Value(String::from("a")), String::from("b")))])
                    )
                ])
            )
        );

    // run resolve on reduced expression with full property set - expect true and no unreduced expressions

    match resolve_result_partial {
        ResolveResult::Undefined(_, unreduced_expr) => {
            let resolve_result_full = unreduced_expr.resolve(&property_set_full);
            assert_eq!(resolve_result_full, ResolveResult::True);
        },
        _ => {
            panic!("Not expected");
        }

    }

}
