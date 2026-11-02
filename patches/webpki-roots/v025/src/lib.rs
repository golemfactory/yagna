//! The webpki-roots 0.25 API backed by the maintained Mozilla CA bundle.
#![no_std]
#![forbid(unsafe_code)]

/// A trust anchor in the representation expected by legacy TLS clients.
pub struct TrustAnchor<'a> {
    pub subject: &'a [u8],
    pub spki: &'a [u8],
    pub name_constraints: Option<&'a [u8]>,
}

include!(concat!(env!("OUT_DIR"), "/roots.rs"));
