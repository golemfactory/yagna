extern crate market_api;

use market_api::resolver::prop_parser::*;

#[test]
fn parse_prop_def_for_simple_prop() {
    assert_eq!(parse_prop_def("prop=value"), Ok(("prop", Some("value"))));
}

#[test]
fn parse_prop_def_for_prop_with_type() {
    assert_eq!(parse_prop_def("prop:Type=value"), Ok(("prop:Type", Some("value"))));
}

#[test]
fn parse_prop_def_for_dynamic_prop() {
    assert_eq!(parse_prop_def("prop*"), Ok(("prop*", None)));
}

#[test]
fn parse_prop_ref_with_aspect_no_aspect() {
    assert_eq!(parse_prop_ref_with_aspect("prop"), Ok(("prop", None)));
}

#[test]
fn parse_prop_ref_with_aspect_syntax_error_1() {
    assert_eq!(parse_prop_ref_with_aspect("prop:asda"), Err("Parsing error: unexpected text :asda".to_string()));
}

#[test]
#[ignore]
fn parse_prop_ref_with_aspect_syntax_error_2() {
    // TODO need to handle syntax error differently
    println!("{:?}", parse_prop_ref_with_aspect("prop[asda"));
    assert_eq!(parse_prop_ref_with_aspect("prop[[asda]"), Ok(("pro p", None)));
}

#[test]
fn parse_prop_ref_with_aspect_simple() {
    assert_eq!(parse_prop_ref_with_aspect("prop[aspect]"), Ok(("prop", Some("aspect"))));
}

#[test]
fn parse_prop_value_from_literal_string() {
    assert_eq!(parse_prop_value_literal("\"dblah\""), Ok(Literal::Str("dblah")));
}

#[test]
fn parse_prop_value_from_literal_datetime() {
    assert_eq!(parse_prop_value_literal("t\"dblah\""), Ok(Literal::DateTime("dblah")));
}

#[test]
fn parse_prop_value_from_literal_version() {
    assert_eq!(parse_prop_value_literal("v\"dblah\""), Ok(Literal::Version("dblah")));
}

#[test]
fn parse_prop_value_from_literal_bool_true() {
    assert_eq!(parse_prop_value_literal("true"), Ok(Literal::Bool(true)));
}

#[test]
fn parse_prop_value_from_literal_bool_false() {
    assert_eq!(parse_prop_value_literal("false"), Ok(Literal::Bool(false)));
}

#[test]
fn parse_prop_value_from_literal_number() {
    assert_eq!(parse_prop_value_literal("124.67"), Ok(Literal::Number("124.67")));
}

#[test]
fn parse_prop_value_from_literal_number_int() {
    assert_eq!(parse_prop_value_literal("124"), Ok(Literal::Number("124")));
}

#[test]
fn parse_prop_value_from_literal_number_error() {
    assert_eq!(parse_prop_value_literal("124asdas234"), Err(String::from("Unknown literal type: 124asdas234")));
}

#[test]
fn parse_prop_value_from_literal_list_string() {
    assert_eq!(parse_prop_value_literal("[\"abc\",\"def\"]"), Ok(Literal::List(
        vec![
            Box::new(Literal::Str("abc")), 
            Box::new(Literal::Str("def"))
            ]
        )));
}

#[test]
fn parse_prop_value_from_literal_list_empty() {
    assert_eq!(parse_prop_value_literal("[]"), Ok(Literal::List(
        vec![]
        )));
}

#[test]
fn parse_prop_value_from_literal_list_single_string() {
    assert_eq!(parse_prop_value_literal("[\"abc\"]"), Ok(Literal::List(
        vec![
            Box::new(Literal::Str("abc"))
            ]
        )));
}
