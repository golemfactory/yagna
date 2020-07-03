use ya_market_resolver::resolver::expression::*;
use ya_market_resolver::resolver::ldap_parser::parse;
use ya_market_resolver::resolver::properties::*;

fn run_resolve_test_with_aspect(
    expr: &str,
    props: &Vec<&str>,
    aspects: &Vec<(&str, &str, &str)>,
    expect_result: ResolveResult,
) {
    let expression = build_expression(&parse(expr).unwrap()).unwrap();

    let mut properties = vec![];
    for prop in props {
        properties.push(prop.to_string());
    }

    let mut property_set = PropertySet::from_flat_props(&properties);

    for aspect in aspects {
        property_set.set_property_aspect(aspect.0, aspect.1, aspect.2)
    }

    assert_eq!(expression.resolve(&property_set), expect_result);
}

#[test]
fn resolve_present_aspect() {
    let f = "(objectClass[aspect]=*)";

    // test positive

    run_resolve_test_with_aspect(
        f,
        &vec!["objectClass=\"Babs Jensen\""],
        &vec![("objectClass", "aspect", "asp_val")],
        ResolveResult::True,
    );

    // test negative (must return name of unresolved property and aspect)

    run_resolve_test_with_aspect(
        f,
        &vec!["objectClass=\"Dblah\""],
        &vec![],
        ResolveResult::False(
            vec![&PropertyRef::Aspect(
                String::from("objectClass"),
                String::from("aspect"),
                PropertyRefType::Any,
            )],
            Expression::Empty(false),
        ),
    );
    run_resolve_test_with_aspect(
        f,
        &vec!["cn=\"Dblah\""],
        &vec![],
        ResolveResult::False(
            vec![&PropertyRef::Aspect(
                String::from("objectClass"),
                String::from("aspect"),
                PropertyRefType::Any,
            )],
            Expression::Present(PropertyRef::Aspect(
                String::from("objectClass"),
                String::from("aspect"),
                PropertyRefType::Any,
            )),
        ),
    );
}

#[test]
fn resolve_equals_aspect() {
    let f = "(cn[aspect]=asp_value)";

    // test positive

    run_resolve_test_with_aspect(
        f,
        &vec!["cn=\"Babs Jensen\""],
        &vec![("cn", "aspect", "asp_value")],
        ResolveResult::True,
    );

    // test negative

    run_resolve_test_with_aspect(
        f,
        &vec!["cn=\"Babs Jensen\""],
        &vec![("cn", "aspect", "asp_dif_value")],
        ResolveResult::False(vec![], Expression::Empty(false)),
    );

    // test undefined

    run_resolve_test_with_aspect(
        f,
        &vec!["cn=\"Babs Jensen\""],
        &vec![("cn", "aspect2", "asp_value")],
        ResolveResult::Undefined(
            vec![&PropertyRef::Aspect(
                String::from("cn"),
                String::from("aspect"),
                PropertyRefType::Any,
            )],
            Expression::Equals(
                PropertyRef::Aspect(
                    String::from("cn"),
                    String::from("aspect"),
                    PropertyRefType::Any,
                ),
                String::from("asp_value"),
            ),
        ),
    );
    run_resolve_test_with_aspect(
        f,
        &vec!["cncxc=\"Babs Jensen\""],
        &vec![("cncxc", "aspect2", "asp_value")],
        ResolveResult::Undefined(
            vec![&PropertyRef::Aspect(
                String::from("cn"),
                String::from("aspect"),
                PropertyRefType::Any,
            )],
            Expression::Equals(
                PropertyRef::Aspect(
                    String::from("cn"),
                    String::from("aspect"),
                    PropertyRefType::Any,
                ),
                String::from("asp_value"),
            ),
        ),
    );
}

#[test]
fn resolve_not_aspect() {
    let f = "(!(cn[aspect]=asp_value))";

    // test positive

    run_resolve_test_with_aspect(
        f,
        &vec!["cn=\"Babs Jensen\""],
        &vec![("cn", "aspect", "asp_dif_value")],
        ResolveResult::True,
    );

    // test negative

    run_resolve_test_with_aspect(
        f,
        &vec!["cn=\"Tim Howes\""],
        &vec![("cn", "aspect", "asp_value")],
        ResolveResult::False(vec![], Expression::Empty(false)),
    );

    // test undefined

    run_resolve_test_with_aspect(
        f,
        &vec!["cn=\"Dblah\""],
        &vec![("cn", "aspect2", "asp2_dif_value")],
        ResolveResult::Undefined(
            vec![&PropertyRef::Aspect(
                String::from("cn"),
                String::from("aspect"),
                PropertyRefType::Any,
            )],
            Expression::Not(Box::new(Expression::Equals(
                PropertyRef::Aspect(
                    String::from("cn"),
                    String::from("aspect"),
                    PropertyRefType::Any,
                ),
                String::from("asp_value"),
            ))),
        ),
    );
}
