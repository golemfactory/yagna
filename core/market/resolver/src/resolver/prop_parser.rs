use std::str;
use std::string::String;

use nom::digit;
use nom::IResult;

named!(prop_def <&str, &str>,
    do_parse!(
            res: take_till!(is_equal_sign) >>
            char!('=') >>
            (res)
    )
);

named!(aspect <&str, &str>,
    do_parse!(
            res: take_till!(is_delimiter) >>
            (res)
    )

);

named!(prop_ref_type_code <&str, &str>,
    do_parse!(
        tag!("$") >>
        code : alt!(tag!("d") | tag!("v") | tag!("t")) >>
        (code)
    )
);

named!(prop <&str, &str>,
    do_parse!(
            res: take_till!(is_delimiter) >>
            (res)
    )
);

named!(prop_ref_aspect_type <&str, (&str, Option<&str>, Option<&str>)>,
    do_parse!(
        prop : prop >>
        aspect : delimited!(char!('['), aspect, char!(']')) >>
        impl_type : prop_ref_type_code >>
        ((prop, Some(aspect), Some(impl_type)))
    )
);

named!(prop_ref_aspect <&str, (&str, Option<&str>, Option<&str>)>,
    do_parse!(
        prop : prop >>
        aspect : delimited!(char!('['), aspect, char!(']')) >>
        ((prop, Some(aspect), None))
    )
);

named!(prop_ref_type <&str, (&str, Option<&str>, Option<&str>)> ,
    do_parse!(
        prop : prop >>
        impl_type : prop_ref_type_code >>
        ((prop, None, Some(impl_type)))
    )
);

named!(prop_ref_no_type <&str, (&str, Option<&str>, Option<&str>)> ,
    do_parse!(
        prop : prop >>
        ((prop, None, None))
    )
);

#[test]
fn test_prop_ref_set() {
    // Correct cases (should be properly parsed):

    // input[aspect]@d
    assert_eq!(
        prop_ref_aspect_type("input[aspect]$d"),
        IResult::Done("", ("input", Some("aspect"), Some("d")))
    );

    // input[aspect]
    assert_eq!(
        prop_ref_aspect_type("input[aspect]"),
        IResult::Incomplete(nom::Needed::Size(14))
    );
    assert_eq!(
        prop_ref_aspect("input[aspect]"),
        IResult::Done("", ("input", Some("aspect"), None))
    );

    // input@d
    assert_eq!(
        prop_ref_aspect_type("input$d"),
        IResult::Error(nom::ErrorKind::Char)
    );
    assert_eq!(
        prop_ref_aspect("input$d"),
        IResult::Error(nom::ErrorKind::Char)
    );
    assert_eq!(
        prop_ref_type("input$d"),
        IResult::Done("", ("input", None, Some("d")))
    );

    // input
    assert_eq!(
        prop_ref_aspect_type("input"),
        IResult::Incomplete(nom::Needed::Size(6))
    );
    assert_eq!(
        prop_ref_aspect("input"),
        IResult::Incomplete(nom::Needed::Size(6))
    );
    assert_eq!(
        prop_ref_type("input"),
        IResult::Incomplete(nom::Needed::Size(6))
    );
    assert_eq!(
        prop_ref_no_type("input"),
        IResult::Done("", ("input", None, None))
    );

    // Incorrect cases

    // input@dqwe
    assert_eq!(
        prop_ref_aspect_type("input$dqwe"),
        IResult::Error(nom::ErrorKind::Char)
    );
    assert_eq!(
        prop_ref_aspect("input$dqwe"),
        IResult::Error(nom::ErrorKind::Char)
    );
    assert_eq!(
        prop_ref_type("input$dqwe"),
        IResult::Done("qwe", ("input", None, Some("d")))
    );

    // input[aspect]@dqwe
    assert_eq!(
        prop_ref_aspect_type("input[aspect]$dqwe"),
        IResult::Done("qwe", ("input", Some("aspect"), Some("d")))
    );

    // input[aspecdqwe
    assert_eq!(
        prop_ref_aspect_type("input[aspecdqwe"),
        IResult::Incomplete(nom::Needed::Size(16))
    );
}

// #region parser of List in property reference strings

named!(prop_ref_list <&str, Vec<&str>>,
    delimited!(char!('['),
            separated_list!(
                tag!(","),
                ws!(prop_ref_list_item)
            )
            , char!(']')
        )
);

named!(prop_ref_list_item <&str, &str>,
    do_parse!(
            res: take_until_either!("[,]") >>
            (res)
    )
);

// #endregion

// #region parser of property value literals

#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'a> {
    Str(&'a str),
    Number(&'a str),
    Decimal(&'a str),
    Bool(bool),
    Version(&'a str),
    DateTime(&'a str),
    List(Vec<Box<Literal<'a>>>),
}

named!(
    val_literal<Literal>,
    alt!(
        str_literal
            | version_literal
            | datetime_literal
            | true_literal
            | false_literal
            | decimal_literal
            | number_literal
            | list_literal
    )
);

named!(
    list_literal<Literal<'a>>,
    ws!(delimited!(
        char!('['),
        map!(separated_list!(tag!(","), val_literal), |v: Vec<
            Literal<'a>,
        >| {
            Literal::List(v.into_iter().map(Box::new).collect())
        }),
        char!(']')
    ))
);

named!(
    str_literal<Literal>,
    ws!(delimited!(
        char!('"'),
        do_parse!(
            val: escaped!(none_of!(r#"\""#), '\\', one_of!("\"nt0\\"))
                >> (Literal::Str(str::from_utf8(val).unwrap()))
        ),
        char!('"')
    ))
);

named!(
    version_literal<Literal>,
    ws!(delimited!(
        tag!("v\""),
        do_parse!(val: take_until!("\"") >> (Literal::Version(str::from_utf8(val).unwrap()))),
        char!('"')
    ))
);

named!(
    datetime_literal<Literal>,
    ws!(delimited!(
        tag!("t\""),
        do_parse!(val: take_until!("\"") >> (Literal::DateTime(str::from_utf8(val).unwrap()))),
        char!('"')
    ))
);

named!(
    decimal_literal<Literal>,
    ws!(delimited!(
        tag!("d\""),
        do_parse!(val: take_until!("\"") >> (Literal::Decimal(str::from_utf8(val).unwrap()))),
        char!('"')
    ))
);

named!(
    true_literal<Literal>,
    ws!(ws!(map!(
        alt!(tag!("true") | tag!("True") | tag!("TRUE")),
        |val| { Literal::Bool(true) }
    )))
);

named!(
    false_literal<Literal>,
    ws!(ws!(map!(
        alt!(tag!("false") | tag!("False") | tag!("FALSE")),
        |val| { Literal::Bool(false) }
    )))
);

named!(signed_digits<&[u8], (Option<&[u8]>,&[u8])>,
    pair!(
        opt!(alt!(tag!("+") | tag!("-"))),  // maybe sign?
        digit
    )
);

named!(maybe_signed_digits<&[u8],&[u8]>,
    recognize!(signed_digits)
);

named!(floating_point <&[u8],&[u8]>,
    recognize!(
        tuple!(
            maybe_signed_digits,
            opt!(complete!(pair!(
                tag!("."),
                digit
            ))),
            opt!(complete!(pair!(
                alt!(tag!("e") | tag!("E")),
                maybe_signed_digits
            )))
        )
    )
);

named!(
    number_literal<Literal>,
    do_parse!(val: floating_point >> (Literal::Number(str::from_utf8(val).unwrap())))
);

// #endregion

// Parse property definition in the form of:
// <property_name>=<property_value>
// Returns a tuple of (property_name, Option(property_value))
pub fn parse_prop_def(input: &str) -> Result<(&str, Option<&str>), String> {
    let iresult = prop_def(input);

    match iresult {
        IResult::Done(rest, t) => Ok((t, Some(rest))),
        IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind)),
        IResult::Incomplete(_needed) => Ok((input, None)),
    }
}

// Parse property reference string (element of filter expression)
// in the form of:
// <property_name>[<aspect_name>]
// where aspect_name is optional.
// Returns a tuple of (property_name, Option(aspect_name), implied_property_type_code)
pub fn parse_prop_ref_with_aspect(
    input: &str,
) -> Result<(&str, Option<&str>, Option<&str>), String> {
    match prop_ref_aspect_type(input) {
        IResult::Done(rest, t) => {
            if rest.is_empty() {
                Ok(t)
            } else {
                Err(format!(
                    "Parsing aspect type error: unexpected text {}",
                    rest
                ))
            }
        }
        IResult::Incomplete(_needed) => {
            // no type, try parsing ref with aspect alone
            match prop_ref_aspect(input) {
                IResult::Done(rest, t) => {
                    if rest.is_empty() {
                        Ok(t)
                    } else {
                        Err(format!(
                            "Parsing aspect no type error: unexpected text {}",
                            rest
                        ))
                    }
                }
                IResult::Incomplete(_) | IResult::Error(_) => parse_prop_ref_no_aspect(input),
            }
        }

        IResult::Error(_error_kind) => {
            // no aspect, try parsing simple property ref
            parse_prop_ref_no_aspect(input)
        }
    }
}

fn parse_prop_ref_no_aspect(input: &str) -> Result<(&str, Option<&str>, Option<&str>), String> {
    match prop_ref_type(input) {
        IResult::Done(rest, t) => {
            if rest.is_empty() {
                Ok(t)
            } else {
                Err(format!(
                    "Parsing no aspect type error: unexpected text {}",
                    rest
                ))
            }
        }
        IResult::Incomplete(_) | IResult::Error(_) => match prop_ref_no_type(input) {
            IResult::Done(rest, t) => {
                if rest.is_empty() {
                    Ok(t)
                } else {
                    Err(format!(
                        "Parsing no aspect no type error: unexpected text {}",
                        rest
                    ))
                }
            }
            IResult::Incomplete(_) | IResult::Error(_) => {
                panic!("unable to parse simple property");
            }
        },
    }
}

// Parse property reference value as List (element of filter expression)
// in the form of:
// [value1,value2,...]
// Returns a tuple of Vec<&str>
pub fn parse_prop_ref_as_list(input: &str) -> Result<Vec<&str>, String> {
    match prop_ref_list(input) {
        IResult::Done(rest, t) => {
            if rest.is_empty() {
                Ok(t)
            } else {
                Err(format!("Parsing list error: unexpected text {}", rest))
            }
        }
        IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind)),
        IResult::Incomplete(needed) => Err(format!("Incomplete expression: {:?}", needed)),
    }
}

// Parse property value string, detecting the type from literal:
// - anything in "" - String
// - true/false, True/False, TRUE/FALSE - Boolean
// - t"<date string>" - DateTime
// - v"<version string>" - Version
// - anything that parses as float - Number
// - ...anything else - error
pub fn parse_prop_value_literal(input: &str) -> Result<Literal, String> {
    let iresult = val_literal(input.as_bytes());

    match iresult {
        IResult::Done(rest, t) => {
            if rest.is_empty() {
                Ok(t)
            } else {
                Err(format!("Unknown literal type: {}", input))
            }
        }
        IResult::Error(error_kind) => {
            Err(format!("Parsing error: {} in text '{}'", error_kind, input))
        }
        IResult::Incomplete(_needed) => Err(format!("Parsing error: {:?}", _needed)),
    }
}

pub fn is_equal_sign(chr: char) -> bool {
    chr == '='
}

pub fn is_delimiter(chr: char) -> bool {
    chr == '[' || chr == ']' || chr == '$'
}
