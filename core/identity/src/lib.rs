//! Yagna identity management

#![allow(non_local_definitions)] // Due to Diesel macros.

#[macro_use]
extern crate diesel;

pub mod cli;
pub mod service;

mod autoconf;
pub mod dao;
mod db;
mod id_key;
