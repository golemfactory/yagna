use ya_market_resolver::resolver::expression::*;
use ya_market_resolver::resolver::ldap_parser::parse;
use ya_market_resolver::resolver::properties::*;

#[test]
fn resolve_multistep_with_supplemented_props() {
    let f = "(&(|(a=b)(b=c)(c=d))(!(x=dblah)))";

    let properties_partial = vec![
        /*"a=\"b\"",*/ "b=\"x\"".to_string(),
        "c=\"y\"".to_string(),
        "x=\"notdblah\"".to_string(),
    ];
    let properties_full = vec![
        "a=\"b\"".to_string(),
        "b=\"x\"".to_string(),
        "c=\"y\"".to_string(),
        "x=\"notdblah\"".to_string(),
    ];

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    let property_set_partial = PropertySet::from_flat_props(&properties_partial);
    let property_set_full = PropertySet::from_flat_props(&properties_full);

    // run resolve for incomplete props - expect unresolved property refs and a reduced expression

    let resolve_result_partial = expression.resolve(&property_set_partial);

    assert_eq!(
        resolve_result_partial,
        ResolveResult::Undefined(
            vec![&PropertyRef::Value(String::from("a"), PropertyRefType::Any)],
            Expression::Equals(
                PropertyRef::Value(String::from("a"), PropertyRefType::Any),
                String::from("b")
            )
        )
    );

    // run resolve on reduced expression with full property set - expect true and no unreduced expressions

    match resolve_result_partial {
        ResolveResult::Undefined(_, unreduced_expr) => {
            let resolve_result_full = unreduced_expr.resolve(&property_set_full);
            assert_eq!(resolve_result_full, ResolveResult::True);
        }
        _ => {
            panic!("Not expected");
        }
    }
}

#[test]
fn resolve_api_with_supplemented_props() {
    let f = "(&(|(a=b)(b=c)(c=d))(!(x=dblah)))";

    let properties_partial = vec![
        /*"a=\"b\"",*/ "b=\"x\"".to_string(),
        "c=\"y\"".to_string(),
        "x=\"notdblah\"".to_string(),
    ];
    let properties_full = vec![
        "a=\"b\"".to_string(),
        "b=\"x\"".to_string(),
        "c=\"y\"".to_string(),
        "x=\"notdblah\"".to_string(),
    ];

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    let property_set_partial = PropertySet::from_flat_props(&properties_partial);
    let property_set_full = PropertySet::from_flat_props(&properties_full);

    // run resolve for incomplete props - expect unresolved property refs and a reduced expression

    let resolve_result_partial = expression.resolve_api(&property_set_partial);

    assert_eq!(resolve_result_partial, Ok(None));

    // run resolve on reduced expression with full property set - expect true and no unreduced expressions

    match expression.resolve_reduce(&property_set_partial) {
        Ok(expr) => {
            assert_eq!(expr.resolve_api(&property_set_full), Ok(Some(true)));
        }
        e => panic!("Unexpected result, got: {:?}", e),
    }
}

#[test]
fn property_refs_returns_correct_result() {
    let f = "(&(|(a=b)(b=c)(c=d))(!(x=dblah)))";

    let expression = build_expression(&parse(f).unwrap()).unwrap();

    // extract property_refs from expression

    assert_eq!(
        expression
            .property_refs()
            .into_iter()
            .collect::<Vec<&PropertyRef>>(),
        vec![
            &PropertyRef::Value("a".to_string(), PropertyRefType::Any),
            &PropertyRef::Value("b".to_string(), PropertyRefType::Any),
            &PropertyRef::Value("c".to_string(), PropertyRefType::Any),
            &PropertyRef::Value("x".to_string(), PropertyRefType::Any),
        ]
    );
}
