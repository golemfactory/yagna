use std::str;

use nom::IResult;

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


named!(prop_with_type <&str, (&str, Option<&str>)>, 
    tuple!(
        prop,
        opt!(preceded!(char!(':'), type_name))
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

// Parse property definition in the form of:
// <property_name_and_type>=<property_value>
// Returns a tuple of (property_name_and_type, Option(property_value))
pub fn parse_prop_def(input : &str) -> Result<(&str, Option<&str>), String>
{
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
            IResult::Incomplete(needed) => {
                println!("Incomplete: {:?}", needed);
                match prop_with_nothing(input) {
                    IResult::Done(_, t) => Ok((t.0, None)),
                    IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
                    IResult::Incomplete(needed) => Err(format!("Incomplete expression: {:?}", needed)),
                }
            },
    }

}

// Parse property declaration string (element of property definition)
// in the form of:
// <property_name>:<type_name>]
// where type_name is optional.
// Returns a tuple of (property_name, Option(type_name))
pub fn parse_prop_ref_with_type(input : &str) -> Result<(&str, Option<&str>), String>
{
        match prop_with_type(input) {
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

pub fn is_equal_sign(chr: char) -> bool {
    chr == '='   
}

pub fn is_delimiter(chr: char) -> bool {
    chr == '[' ||
    chr == ']' ||
    chr == ':'  
}
