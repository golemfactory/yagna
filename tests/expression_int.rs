extern crate nom;
extern crate asnom;
extern crate market_api;

use market_api::resolver::*;
use market_api::resolver::properties::*;
use market_api::resolver::ldap_parser::parse;
use market_api::resolver::expression::*;

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
                vec![&PropertyRef::Value(String::from("a"), PropertyRefType::Any)],
                Expression::And(vec![
                    Box::new(
                        Expression::Or(vec![Box::new(Expression::Equals(PropertyRef::Value(String::from("a"), PropertyRefType::Any), String::from("b")))])
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
