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
    assert_eq!(parse_prop_value_literal("\"dblah\""), Ok((TAG_STRING, "dblah")));
}

#[test]
fn parse_prop_value_from_literal_datetime() {
    assert_eq!(parse_prop_value_literal("t\"dblah\""), Ok((TAG_DATETIME, "dblah")));
}

#[test]
fn parse_prop_value_from_literal_version() {
    assert_eq!(parse_prop_value_literal("v\"dblah\""), Ok((TAG_VERSION, "dblah")));
}

#[test]
fn parse_prop_value_from_literal_bool_true() {
    assert_eq!(parse_prop_value_literal("true"), Ok((TAG_BOOLEAN_TRUE, "true")));
}

#[test]
fn parse_prop_value_from_literal_bool_false() {
    assert_eq!(parse_prop_value_literal("false"), Ok((TAG_BOOLEAN_FALSE, "false")));
}

#[test]
fn parse_prop_value_from_literal_number() {
    assert_eq!(parse_prop_value_literal("124.67"), Ok((TAG_NUMBER, "124.67")));
}

#[test]
fn parse_prop_value_from_literal_number_int() {
    assert_eq!(parse_prop_value_literal("124"), Ok((TAG_NUMBER, "124")));
}

#[test]
fn parse_prop_value_from_literal_number_error() {
    assert_eq!(parse_prop_value_literal("124asdas234"), Err(String::from("Unknown literal type: 124asdas234")));
}

/*#[test]
fn parse_prop_ref_with_type_no_type() {
    assert_eq!(parse_prop_ref_with_type("prop"), Ok(("prop", None)));
}

#[test]
fn parse_prop_ref_with_type_simple_type() {
    assert_eq!(parse_prop_ref_with_type("prop:type"), Ok(("prop", Some("type"))));
}

#[test]
fn parse_prop_ref_with_type_syntax_error() {
    assert_eq!(parse_prop_ref_with_type("prop[type"), Err("Parsing error: unexpected text [type".to_string()));
}
*/