//! The webpki-roots 0.22 API backed by the maintained Mozilla CA bundle.
#![no_std]
#![forbid(unsafe_code)]

include!(concat!(env!("OUT_DIR"), "/roots.rs"));
