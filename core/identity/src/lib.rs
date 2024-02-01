#![allow(clippy::arc_with_non_send_sync)]

/// Yagna identity management

#[macro_use]
extern crate diesel;

pub mod cli;
pub mod service;

mod autoconf;
pub mod dao;
mod db;
mod id_key;
