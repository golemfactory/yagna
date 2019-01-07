#[macro_use]
extern crate nom;

use nom::IResult;

named!(with_nothing <&str, (&str, Option<&str>)>, 
    tuple!(
        prop,
        opt!(end)
    )
);

named!(with_aspect <&str, (&str, Option<&str>)>, 
    tuple!(
        prop,
        opt!(delimited!(char!('['), asp, char!(']')))
    )
);


named!(with_type <&str, (&str, Option<&str>)>, 
    tuple!(
        prop,
        opt!(preceded!(char!(':'), typ))
    )
);

named!(prop <&str, &str>, 
    do_parse!(
            res: take_till!(is_delimiter) >>
            (res)
    )
);

named!(asp <&str, &str>, 
    do_parse!(
            res: take_till!(is_delimiter) >>
            (res)
    )

);

named!(typ <&str, &str>, 
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

fn parse_with_aspect(input : &str) -> Result<(&str, Option<&str>), String>
{
        match with_aspect(input) {
            IResult::Done(_, t) => Ok(t),
            IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
            IResult::Incomplete(needed) => {
                match with_nothing(input) {
                    IResult::Done(_, t) => Ok((t.0, None)),
                    IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
                    IResult::Incomplete(needed) => Err(format!("Incomplete expression: {:?}", needed)),
                }
            },
    }

}

fn parse_with_type(input : &str) -> Result<(&str, Option<&str>), String>
{
        match with_type(input) {
            IResult::Done(_, t) => Ok(t),
            IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
            IResult::Incomplete(needed) => {
                match with_nothing(input) {
                    IResult::Done(_, t) => Ok((t.0, None)),
                    IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind.to_string())),
                    IResult::Incomplete(needed) => Err(format!("Incomplete expression: {:?}", needed)),
                }
            },
    }

}

#[test]
fn present() {
    //let f = "prop[aspect]:type";
    let a = "prop[aspect]";
    let b = "prop:type";
    let c = "prop";

    println!("{:?}", parse_with_aspect(a));
    println!("{:?}", parse_with_type(b));
    println!("{:?}", parse_with_aspect(c));
    println!("{:?}", parse_with_type(c));
    assert!(false);
    //assert_eq!(prop(f), IResult::Ok());
}

pub fn is_delimiter(chr: char) -> bool {
    chr == '['  ||
    chr == ']'  ||
    chr == ':'  
}
