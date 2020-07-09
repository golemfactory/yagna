use ya_market_resolver::resolver::ldap_parser::*;

use asnom::common::TagClass;
use asnom::structures::{ExplicitTag, OctetString, Sequence, Tag};
use std::default::Default;

#[test]
fn present() {
    let f = "(objectClass=*)";

    let tag = Tag::OctetString(OctetString {
        class: TagClass::Context,
        id: TAG_PRESENT,
        inner: vec![
            0x6f, 0x62, 0x6a, 0x65, 0x63, 0x74, 0x43, 0x6c, 0x61, 0x73, 0x73,
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn simple() {
    let f = "(cn=Babs Jensen)";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: TAG_EQUAL,
        inner: vec![
            Tag::OctetString(OctetString {
                inner: vec![0x63, 0x6e],
                ..Default::default()
            }),
            Tag::OctetString(OctetString {
                inner: vec![
                    0x42, 0x61, 0x62, 0x73, 0x20, 0x4a, 0x65, 0x6e, 0x73, 0x65, 0x6e,
                ],
                ..Default::default()
            }),
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn greater() {
    let f = "(cn>Babs Jensen)";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: TAG_GREATER,
        inner: vec![
            Tag::OctetString(OctetString {
                inner: vec![0x63, 0x6e],
                ..Default::default()
            }),
            Tag::OctetString(OctetString {
                inner: vec![
                    0x42, 0x61, 0x62, 0x73, 0x20, 0x4a, 0x65, 0x6e, 0x73, 0x65, 0x6e,
                ],
                ..Default::default()
            }),
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn greater_equal() {
    let f = "(cn>=Babs Jensen)";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: TAG_GREATER_EQUAL,
        inner: vec![
            Tag::OctetString(OctetString {
                inner: vec![0x63, 0x6e],
                ..Default::default()
            }),
            Tag::OctetString(OctetString {
                inner: vec![
                    0x42, 0x61, 0x62, 0x73, 0x20, 0x4a, 0x65, 0x6e, 0x73, 0x65, 0x6e,
                ],
                ..Default::default()
            }),
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn less() {
    let f = "(cn<Babs Jensen)";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: TAG_LESS,
        inner: vec![
            Tag::OctetString(OctetString {
                inner: vec![0x63, 0x6e],
                ..Default::default()
            }),
            Tag::OctetString(OctetString {
                inner: vec![
                    0x42, 0x61, 0x62, 0x73, 0x20, 0x4a, 0x65, 0x6e, 0x73, 0x65, 0x6e,
                ],
                ..Default::default()
            }),
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn less_equal() {
    let f = "(cn<=Babs Jensen)";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: TAG_LESS_EQUAL,
        inner: vec![
            Tag::OctetString(OctetString {
                inner: vec![0x63, 0x6e],
                ..Default::default()
            }),
            Tag::OctetString(OctetString {
                inner: vec![
                    0x42, 0x61, 0x62, 0x73, 0x20, 0x4a, 0x65, 0x6e, 0x73, 0x65, 0x6e,
                ],
                ..Default::default()
            }),
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn not() {
    let f = "(!(cn=Tim Howes))";

    let tag = Tag::ExplicitTag(ExplicitTag {
        class: TagClass::Context,
        id: TAG_NOT,
        inner: Box::new(Tag::Sequence(Sequence {
            class: TagClass::Context,
            id: TAG_EQUAL,
            inner: vec![
                Tag::OctetString(OctetString {
                    inner: vec![0x63, 0x6e],
                    ..Default::default()
                }),
                Tag::OctetString(OctetString {
                    inner: vec![0x54, 0x69, 0x6d, 0x20, 0x48, 0x6f, 0x77, 0x65, 0x73],
                    ..Default::default()
                }),
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
        id: TAG_NOT,
        inner: Box::new(Tag::Sequence(Sequence {
            class: TagClass::Context,
            id: TAG_EQUAL,
            inner: vec![
                Tag::OctetString(OctetString {
                    inner: vec![0x63, 0x6e],
                    ..Default::default()
                }),
                Tag::OctetString(OctetString {
                    inner: vec![0x54, 0x69, 0x6d, 0x20, 0x48, 0x6f, 0x77, 0x65, 0x73],
                    ..Default::default()
                }),
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
        id: TAG_AND,
        inner: vec![
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: TAG_EQUAL,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x61],
                        ..Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x62],
                        ..Default::default()
                    }),
                ],
            }),
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: TAG_EQUAL,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x62],
                        ..Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x63],
                        ..Default::default()
                    }),
                ],
            }),
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: TAG_EQUAL,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x63],
                        ..Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x64],
                        ..Default::default()
                    }),
                ],
            }),
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}

#[test]
fn and_whitespace() {
    let f = "( &  (a=b)  (b=c)  (c=d) )";

    let tag = Tag::Sequence(Sequence {
        class: TagClass::Context,
        id: TAG_AND,
        inner: vec![
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: TAG_EQUAL,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x61],
                        ..Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x62],
                        ..Default::default()
                    }),
                ],
            }),
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: TAG_EQUAL,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x62],
                        ..Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x63],
                        ..Default::default()
                    }),
                ],
            }),
            Tag::Sequence(Sequence {
                class: TagClass::Context,
                id: TAG_EQUAL,
                inner: vec![
                    Tag::OctetString(OctetString {
                        inner: vec![0x63],
                        ..Default::default()
                    }),
                    Tag::OctetString(OctetString {
                        inner: vec![0x64],
                        ..Default::default()
                    }),
                ],
            }),
        ],
    });

    assert_eq!(parse(f), Ok(tag));
}
