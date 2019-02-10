use std::str;

use nom::IResult;
use nom::digit;

// Tag constants

pub const TAG_STRING : u64 = 0;
pub const TAG_NUMBER : u64 = 1;
pub const TAG_BOOLEAN_TRUE : u64 = 2;
pub const TAG_BOOLEAN_FALSE : u64 = 3;
pub const TAG_DATETIME : u64 = 4;
pub const TAG_VERSION : u64 = 5;

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

// parser of property value literals
named!(val_literal <(u64, &[u8])>, alt!( string_literal | version_literal | datetime_literal | true_literal | false_literal | number_literal ) );

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

named!( string_literal <(u64, &[u8])>, ws!(
    delimited!(
        char!('"'), 
        do_parse!(val : take_until!("\"") >>
            (TAG_STRING, val)
        ),
        char!('"')
    )
));

named!( version_literal <(u64, &[u8])>, ws!(
    delimited!(
        tag!("v\""), 
        do_parse!(val : take_until!("\"") >>
            (TAG_VERSION, val)
        ),
        char!('"')
    )
));

named!( datetime_literal <(u64, &[u8])>, ws!(
    delimited!(
        tag!("t\""), 
        do_parse!(val : take_until!("\"") >>
            (TAG_DATETIME, val)
        ),
        char!('"')
    )
));

named!( true_literal <(u64, &[u8])>, ws!(
    ws!(
        map!(
            alt!(tag!("true") | tag!("True") | tag!("TRUE")),
            |val| -> (u64, &[u8]) { (TAG_BOOLEAN_TRUE, val) }
        )
    )
));

named!( false_literal <(u64, &[u8])>, ws!(
    ws!(
        map!(
            alt!(tag!("false") | tag!("False") | tag!("FALSE")),
            |val| -> (u64, &[u8]) { (TAG_BOOLEAN_FALSE, val) }
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

named!(number_literal <(u64, &[u8])> ,
    do_parse!(
        val : floating_point >>
        (TAG_NUMBER, val)
    )
);


// Parse property definition in the form of:
// <property_name>=<property_value>
// Returns a tuple of (property_name, Option(property_value))
pub fn parse_prop_def(input : &str) -> Result<(&str, Option<&str>), String>
{
    println!("parse_prop_def: {}", input);
    
    match prop_def(input) {
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

// Parse property value string, detecting the type from literal:
// - anything in "" - String
// - true/false, True/False, TRUE/FALSE - Boolean
// - t"<date string>" - DateTime
// - v"<version string>" - Version
// - anything that parses as float - Number
// - ...anything else - error
pub fn parse_prop_value_literal(input : &str) -> Result<(u64, &str), String>
{
    match val_literal(input.as_bytes()) {
        IResult::Done(rest, t) => 
            if rest.len() == 0 { 
                Ok((t.0, str::from_utf8(t.1).unwrap())) 
            } 
            else {
                Err(format!("Unknown literal type: {}", input))
            },
            IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
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
