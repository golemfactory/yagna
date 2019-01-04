extern crate nom;
extern crate asnom;
extern crate market_api;

use market_api::resolver::ldap_parser::parse;

use std::default::Default;
use asnom::common::TagClass;
use asnom::structures::{Tag, OctetString, Sequence, ExplicitTag};

#[test]
fn present() {
    let f = "(objectClass=*)";

    let tag = Tag::OctetString(OctetString {
        class: TagClass::Context,
        id: 7,
        inner: vec![
            0x6f, 0x62, 0x6a, 0x65, 0x63, 0x74, 0x43, 0x6c, 0x61, 0x73, 0x73
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn simple() {
    let f = "(cn=Babs Jensen)";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: 3,
        inner: vec![
                Tag::OctetString(OctetString {
                    inner: vec![0x63, 0x6e],
                    .. Default::default()
                }),
                Tag::OctetString(OctetString {
                    inner: vec![0x42, 0x61, 0x62, 0x73, 0x20, 0x4a, 0x65, 0x6e, 0x73, 0x65, 0x6e],
                    .. Default::default()
                })
        ]
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn not() {
    let f = "(!(cn=Tim Howes))";

    let tag = Tag::ExplicitTag(ExplicitTag {
        class: TagClass::Context,
        id: 2,
        inner: Box::new(Tag::Sequence(Sequence {
            class: TagClass::Context,
            id: 3,
            inner: vec![
                Tag::OctetString(OctetString {
                    inner: vec![0x63, 0x6e],
                    .. Default::default()
                }),
                Tag::OctetString(OctetString {
                    inner: vec![0x54, 0x69, 0x6d, 0x20, 0x48, 0x6f, 0x77, 0x65, 0x73],
                    .. Default::default()
                })
            ],
        })),
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn not_whitespace() {
    let f = "( !   (cn=Tim Howes))";

    let tag = Tag::ExplicitTag(ExplicitTag {
        class: TagClass::Context,
        id: 2,
        inner: Box::new(Tag::Sequence(Sequence {
            class: TagClass::Context,
            id: 3,
            inner: vec![
                Tag::OctetString(OctetString {
                    inner: vec![0x63, 0x6e],
                    .. Default::default()
                }),
                Tag::OctetString(OctetString {
                    inner: vec![0x54, 0x69, 0x6d, 0x20, 0x48, 0x6f, 0x77, 0x65, 0x73],
                    .. Default::default()
                })
            ],
        })),
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn and() {
    let f = "(&(a=b)(b=c)(c=d))";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: 0,
        inner: vec![
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: 3,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x61],
                        .. Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x62],
                        .. Default::default()
                    })
                ]
            }),
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: 3,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x62],
                        .. Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x63],
                        .. Default::default()
                    })
                ]
            }),
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: 3,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x63],
                        .. Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x64],
                        .. Default::default()
                    })
                ]
            }),
        ]
    });

    assert_eq!(parse(f), Ok(tag));
}


#[test]
fn and_whitespace() {
    let f = "( &  (a=b)  (b=c)  (c=d) )";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: 0,
        inner: vec![
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: 3,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x61],
                        .. Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x62],
                        .. Default::default()
                    })
                ]
            }),
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: 3,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x62],
                        .. Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x63],
                        .. Default::default()
                    })
                ]
            }),
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: 3,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x63],
                        .. Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x64],
                        .. Default::default()
                    })
                ]
            }),
        ]
    });

    assert_eq!(parse(f), Ok(tag));
}
