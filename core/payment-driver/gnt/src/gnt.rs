use std::future::Future;
use std::pin::Pin;

pub mod common;
pub mod config;
pub mod ethereum;
pub mod faucet;
pub mod sender;

pub type SignTx<'a> = &'a (dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>);
