use std::default::Default;

use nom::IResult;

use asnom::common::TagClass;
use asnom::structures::{ExplicitTag, Null, OctetString, Sequence, Tag};

// Tag constants

pub const TAG_EMPTY: u64 = 0;
pub const TAG_AND: u64 = 0;
pub const TAG_OR: u64 = 1;
pub const TAG_NOT: u64 = 2;
pub const TAG_EQUAL: u64 = 3;
pub const TAG_PRESENT: u64 = 7;
pub const TAG_GREATER: u64 = 8;
pub const TAG_GREATER_EQUAL: u64 = 9;
pub const TAG_LESS: u64 = 10;
pub const TAG_LESS_EQUAL: u64 = 11;

// Parse function

pub fn parse(input: &str) -> Result<Tag, String> {
    match filter(input.as_bytes()) {
        IResult::Done(_, t) => Ok(t),
        IResult::Error(error_kind) => Err(format!("Parsing error: {}", error_kind)),
        IResult::Incomplete(needed) => Err(format!("Incomplete expression: {:?}", needed)),
    }
}

named!(
    filter<Tag>,
    alt!(match_empty | ws!(delimited!(char!('('), content, char!(')'))))
);
named!(filterlist<Vec<Tag>>, many0!(filter));
named!(content<Tag>, alt!(and | or | not | match_f));

named!(
    and<Tag>,
    map!(
        preceded!(ws!(char!('&')), filterlist),
        |tagv: Vec<Tag>| -> Tag {
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: TAG_AND,
                inner: tagv,
            })
        }
    )
);
named!(
    or<Tag>,
    map!(
        preceded!(ws!(char!('|')), filterlist),
        |tagv: Vec<Tag>| -> Tag {
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: TAG_OR,
                inner: tagv,
            })
        }
    )
);
named!(
    not<Tag>,
    map!(preceded!(ws!(char!('!')), filter), |tag: Tag| -> Tag {
        Tag::ExplicitTag(ExplicitTag {
            class: TagClass::Context,
            id: TAG_NOT,
            inner: Box::new(tag),
        })
    })
);

named!(
    match_empty<Tag>,
    map!(tag!("()"), |_| {
        Tag::Null(Null {
            class: TagClass::Context,
            id: TAG_EMPTY,
            inner: (),
        })
    })
);

named!(match_f<Tag>, alt!(present | simple));

named!(
    present<Tag>,
    do_parse!(
        attr: take_till!(is_delimiter)
            >> tag!("=*")
            >> (Tag::OctetString(OctetString {
                class: TagClass::Context,
                id: TAG_PRESENT,
                inner: attr.to_vec(),
            }))
    )
);

named!(
    simple<Tag>,
    do_parse!(
        attr: take_till!(is_delimiter)
            >> filtertype: filtertype
            >> value: take_until!(")")
            >> (Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: filtertype,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: attr.to_vec(),
                        ..Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: value.to_vec(),
                        ..Default::default()
                    })
                ]
            }))
    )
);

//named!(filtertype <u64>, call!(equal));

named!(
    filtertype<u64>,
    alt!(equal | less_equal | less | greater_equal | greater)
);

named!(equal<u64>, do_parse!(char!('=') >> (TAG_EQUAL)));

named!(less<u64>, do_parse!(char!('<') >> (TAG_LESS)));

named!(less_equal<u64>, do_parse!(tag!("<=") >> (TAG_LESS_EQUAL)));

named!(greater<u64>, do_parse!(char!('>') >> (TAG_GREATER)));

named!(
    greater_equal<u64>,
    do_parse!(tag!(">=") >> (TAG_GREATER_EQUAL))
);

pub fn is_delimiter(chr: u8) -> bool {
    chr == b'=' as u8 || chr == b'<' as u8 || chr == b'>' as u8 || chr == b'~' as u8
}
