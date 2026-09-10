//! Compatibility name for the historical parser module.
//!
//! Yagna constraints resemble LDAP filters, but they are parsed directly into
//! the resolver's flat expression representation and no longer use ASN.1 tags.

pub use super::constraint_parser::{
    parse, parse_with_limits, Limit, ParseError, ParseErrorKind, ParseLimits,
};
