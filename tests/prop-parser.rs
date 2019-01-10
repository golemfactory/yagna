extern crate market_api;

use market_api::resolver::prop_parser::*;

#[test]
fn parse_prop_def_for_simple_prop() {
    assert_eq!(parse_prop_def("prop=value"), Ok(("prop", Some("value"))));
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
