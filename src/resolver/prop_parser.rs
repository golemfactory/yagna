use std::default::Default;
use std::str;

use nom::IResult;

pub struct PropRef {
    pub name: Vec<u8>,
    pub aspect: Option<Vec<u8>>,
    pub type_name: Option<Vec<u8>>
}


// Parse function

pub fn parse(input: &str) -> Result<(&str, Option<&str>, Option<&str>), String> {
    match prop(input.as_bytes()) {
        IResult::Done(_, t) => 
        {   
            // let result = (str::from_utf8(&t.name).unwrap(),
            //      match t.aspect {
            //          Some(a) => Some(str::from_utf8(&a).unwrap()),
            //          None => None
            //      },
            //      match t.type_name {
            //          Some(a) => Some(str::from_utf8(&a).unwrap()),
            //          None => None
            //      },
            //   );

            let result = ("", None, None);
            Ok(result)
        },
        IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
        IResult::Incomplete(needed) => Err(format!("Incomplete expression: {:?}", needed)),
    }
}


named!(prop <PropRef> , 
        do_parse!(
            nam: take_till!(is_delimiter) >>
            asp: take_till!(is_delimiter) >> 
            type_name: eof!() >> 
            (
                PropRef {
                    name:nam.to_vec(), 
                    aspect: Some(asp.to_vec()), 
                    type_name: Some(type_name.to_vec())
                }
            )
        )
    );



pub fn is_delimiter(chr: u8) -> bool {
    chr == '[' as u8 ||
    chr == ']' as u8 ||
    chr == ':' as u8 
}
