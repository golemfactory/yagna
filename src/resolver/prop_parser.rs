use std::str;

use nom::IResult;
use nom::digit;

named!(prop_def <&str, &str>, 
    do_parse!(
            res: take_till!(is_equal_sign) >> 
            char!('=') >>
            (res)
    )
);

named!(prop_with_nothing <&str, (&str, Option<&str>)>, 
    tuple!(
        prop,
        opt!(end)
    )
);

named!(prop_with_aspect <&str, (&str, Option<&str>)>, 
    tuple!(
        prop,
        opt!(delimited!(char!('['), aspect, char!(']')))
    )
);

named!(prop <&str, &str>, 
    do_parse!(
            res: take_till!(is_delimiter) >>
            (res)
    )
);

named!(aspect <&str, &str>, 
    do_parse!(
            res: take_till!(is_delimiter) >>
            (res)
    )

);

named!(type_name <&str, &str>, 
    do_parse!(
            res: take_till!(is_delimiter) >> 
            (res)
    )

);

named!(end <&str, &str >, 
    do_parse!(
            eof!() >> 
            ("")
    )
);

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

named!(val_literal <Literal>, alt!( 
    string_literal | 
    version_literal | 
    datetime_literal | 
    true_literal | 
    false_literal |
    decimal_literal | 
    number_literal |
    list_literal
    ) );

// named!( string_literal <(u64, &[u8])>, ws!(
//     delimited!(
//         char!('"'), 
//         map!(
//             many0!(
//                 none_of!("\"")
//             ),
//              // Make a string from a vector of chars
//             |v : Vec<char>| -> (u64, &[u8]) { (TAG_STRING, v.iter().collect::<String>()) }
//         ),
//         char!('"')
//     )
// ));


named!( list_literal <Literal<'a>>, ws!(
    delimited!(
        char!('['), 
        map!(
            separated_list!(
                tag!(","),
                val_literal
            ),
            |v : Vec<Literal<'a>>| { 
                
                Literal::List(
                    v.into_iter().map(|item| { Box::new(item) }).collect()
                )
            }
        ),
        char!(']')
    )
));

named!( string_literal <Literal>, ws!(
    delimited!(
        char!('"'), 
        do_parse!(val : take_until!("\"") >>
            (Literal::Str(str::from_utf8(val).unwrap()))
        ),
        char!('"')
    )
));

named!( version_literal <Literal>, ws!(
    delimited!(
        tag!("v\""), 
        do_parse!(val : take_until!("\"") >>
            (Literal::Version(str::from_utf8(val).unwrap()))
        ),
        char!('"')
    )
));

named!( datetime_literal <Literal>, ws!(
    delimited!(
        tag!("t\""), 
        do_parse!(val : take_until!("\"") >>
            (Literal::DateTime(str::from_utf8(val).unwrap()))
        ),
        char!('"')
    )
));

named!( decimal_literal <Literal>, ws!(
    delimited!(
        tag!("d\""), 
        do_parse!(val : take_until!("\"") >>
            (Literal::Decimal(str::from_utf8(val).unwrap()))
        ),
        char!('"')
    )
));

named!( true_literal <Literal>, ws!(
    ws!(
        map!(
            alt!(tag!("true") | tag!("True") | tag!("TRUE")),
            |val| { (Literal::Bool(true)) }
        )
    )
));

named!( false_literal <Literal>, ws!(
    ws!(
        map!(
            alt!(tag!("false") | tag!("False") | tag!("FALSE")),
            |val| { Literal::Bool(false) }
        )
    )
));


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

named!(number_literal <Literal> ,
    do_parse!(
        val : floating_point >>
        (Literal::Number(str::from_utf8(val).unwrap()))
    )
);

// #endregion

// Parse property definition in the form of:
// <property_name>=<property_value>
// Returns a tuple of (property_name, Option(property_value))
pub fn parse_prop_def(input : &str) -> Result<(&str, Option<&str>), String>
{
    let iresult = prop_def(input);

    match iresult {
        IResult::Done(rest, t) => Ok((t, Some(rest))),
        IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
        IResult::Incomplete(_needed) => {
            Ok((input, None))
        }
    }
}

// Parse property reference string (element of filter expression)
// in the form of:
// <property_name>[<aspect_name>]
// where aspect_name is optional.
// Returns a tuple of (property_name, Option(aspect_name))
pub fn parse_prop_ref_with_aspect(input : &str) -> Result<(&str, Option<&str>), String>
{
    match prop_with_aspect(input) {
            IResult::Done(rest, t) => if rest == "" { Ok(t) } else { Err(format!("Parsing error: unexpected text {}", rest)) },
            IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
            IResult::Incomplete(_needed) => {
                match prop_with_nothing(input) {
                    IResult::Done(_, t) => Ok((t.0, None)),
                    IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
                    IResult::Incomplete(needed) => Err(format!("Incomplete expression: {:?}", needed)),
                }
            },
    }

}

// Parse property reference value as List (element of filter expression)
// in the form of:
// [value1,value2,...]
// Returns a tuple of Vec<&str>
pub fn parse_prop_ref_as_list(input : &str) -> Result<Vec<&str>, String>
{
    match prop_ref_list(input) {
            IResult::Done(rest, t) => if rest == "" { Ok(t) } else { Err(format!("Parsing error: unexpected text {}", rest)) },
            IResult::Error(error_kind) => {
                println!("Error kind: {:?}", error_kind);
                Err(format!("Parsing error: {}", error_kind.to_string()))
            },
            IResult::Incomplete(needed) => Err(format!("Incomplete expression: {:?}", needed)) 
    }
}

// Parse property value string, detecting the type from literal:
// - anything in "" - String
// - true/false, True/False, TRUE/FALSE - Boolean
// - t"<date string>" - DateTime
// - v"<version string>" - Version
// - anything that parses as float - Number
// - ...anything else - error
pub fn parse_prop_value_literal(input : &str) -> Result<Literal, String>
{
    let iresult = val_literal(input.as_bytes());

    match iresult {
        IResult::Done(rest, t) => 
            if rest.len() == 0 { 
                Ok(t) 
            } 
            else {
                Err(format!("Unknown literal type: {}", input))
            },
            IResult::Error(error_kind) => {
                Err(format!("Parsing error: {} in text '{}'", error_kind.to_string(), input))
            },
            IResult::Incomplete(_needed) => {
                println!("Needed {:?}", _needed);
                Err(format!("Parsing error: {:?}", _needed))
        },
    }

}

pub fn is_equal_sign(chr: char) -> bool {
    chr == '='   
}

pub fn is_delimiter(chr: char) -> bool {
    chr == '[' ||
    chr == ']' ||
    chr == ':'  
}

