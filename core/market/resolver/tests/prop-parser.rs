use ya_market_resolver::resolver::prop_parser::*;

#[test]
fn parse_prop_def_for_simple_prop() {
    assert_eq!(parse_prop_def("prop=value"), Ok(("prop", Some("value"))));
}

#[test]
fn parse_prop_def_for_prop_with_type() {
    assert_eq!(
        parse_prop_def("prop:Type=value"),
        Ok(("prop:Type", Some("value")))
    );
}

#[test]
fn parse_prop_def_for_dynamic_prop() {
    assert_eq!(parse_prop_def("prop*"), Ok(("prop*", None)));
}

#[test]
fn parse_prop_ref_with_aspect_no_aspect_no_impl_type() {
    assert_eq!(parse_prop_ref_with_aspect("prop"), Ok(("prop", None, None)));
}

#[test]
fn parse_prop_ref_with_aspect_no_aspect_impl_type() {
    assert_eq!(
        parse_prop_ref_with_aspect("prop$d"),
        Ok(("prop", None, Some("d")))
    );
}

#[test]
fn parse_prop_ref_with_aspect_syntax_error_1() {
    assert_eq!(
        parse_prop_ref_with_aspect("prop$asda"),
        Err("Parsing no aspect no type error: unexpected text $asda".to_string())
    );
}

#[test]
fn parse_prop_ref_with_aspect_syntax_error_2() {
    assert_eq!(
        parse_prop_ref_with_aspect("prop[[asda]"),
        Err("Parsing no aspect no type error: unexpected text [[asda]".to_string())
    );
}

#[test]
fn parse_prop_ref_with_aspect_simple() {
    assert_eq!(
        parse_prop_ref_with_aspect("prop[aspect]"),
        Ok(("prop", Some("aspect"), None))
    );
}

#[test]
fn parse_prop_ref_as_list_ok() {
    assert_eq!(
        parse_prop_ref_as_list("[prop,  234]"),
        Ok(vec!["prop", "234"])
    );
}

#[test]
fn parse_prop_ref_as_list_with_space_ok() {
    assert_eq!(
        parse_prop_ref_as_list("[pr op,  234]"),
        Ok(vec!["pr op", "234"])
    );
}

#[test]
fn parse_prop_ref_as_list_single_item_ok() {
    assert_eq!(parse_prop_ref_as_list("[prop]"), Ok(vec!["prop"]));
}

#[test]
fn parse_prop_ref_as_list_empty_ok() {
    assert_eq!(parse_prop_ref_as_list("[]"), Ok(vec![]));
}

#[test]
fn parse_prop_ref_as_list_syntax_error() {
    assert_eq!(
        parse_prop_ref_as_list("[prop"),
        Err(String::from("Parsing error: Char"))
    );
}

#[test]
fn parse_prop_ref_as_list_syntax_error2() {
    assert_eq!(
        parse_prop_ref_as_list("asdas[prop,prop2]"),
        Err(String::from("Parsing error: Char"))
    );
}

#[test]
fn parse_prop_value_from_literal_string() {
    assert_eq!(
        parse_prop_value_literal(r#""dblah""#),
        Ok(Literal::Str("dblah"))
    );
}

#[test]
fn parse_prop_value_from_literal_string_with_quotes() {
    assert_eq!(
        parse_prop_value_literal(r#""one \"two\" \tthree\n""#),
        Ok(Literal::Str(r#"one \"two\" \tthree\n"#))
    );
}

#[test]
fn parse_prop() {
    assert_eq!(
        parse_prop_value_literal(
            r#""hash:sha3:aabb:http://repo.some.network:8000/some-image-ddee0011.gvmi\n""#
        ),
        Ok(Literal::Str(
            r#"hash:sha3:aabb:http://repo.some.network:8000/some-image-ddee0011.gvmi\n"#
        ))
    )
}

#[test]
fn parse_prop_value_from_literal_datetime() {
    assert_eq!(
        parse_prop_value_literal("t\"dblah\""),
        Ok(Literal::DateTime("dblah"))
    );
}

#[test]
fn parse_prop_value_from_literal_version() {
    assert_eq!(
        parse_prop_value_literal("v\"dblah\""),
        Ok(Literal::Version("dblah"))
    );
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
fn parse_prop_value_from_literal_decimal() {
    assert_eq!(
        parse_prop_value_literal("d\"124.67\""),
        Ok(Literal::Decimal("124.67"))
    );
}

#[test]
fn parse_prop_value_from_literal_number() {
    assert_eq!(
        parse_prop_value_literal("124.67"),
        Ok(Literal::Number("124.67"))
    );
}

#[test]
fn parse_prop_value_from_literal_number_int() {
    assert_eq!(parse_prop_value_literal("124"), Ok(Literal::Number("124")));
}

#[test]
fn parse_prop_value_from_literal_number_error() {
    assert_eq!(
        parse_prop_value_literal("124asdas234"),
        Err(String::from("Unknown literal type: 124asdas234"))
    );
}

#[test]
fn parse_prop_value_from_literal_list_string() {
    assert_eq!(
        parse_prop_value_literal("[\"abc\",\"def\"]"),
        Ok(Literal::List(vec![
            Box::new(Literal::Str("abc")),
            Box::new(Literal::Str("def"))
        ]))
    );
}

#[test]
fn parse_prop_value_from_literal_list_error() {
    assert_eq!(
        parse_prop_value_literal("[\"abc\",asda33]"),
        Err(String::from(
            "Parsing error: Alternative in text '[\"abc\",asda33]'"
        ))
    );
}

#[test]
fn parse_prop_value_from_literal_list_empty() {
    assert_eq!(parse_prop_value_literal("[]"), Ok(Literal::List(vec![])));
}

#[test]
fn parse_prop_value_from_literal_list_single_string() {
    assert_eq!(
        parse_prop_value_literal("[\"abc\"]"),
        Ok(Literal::List(vec![Box::new(Literal::Str("abc"))]))
    );
}
