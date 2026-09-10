//! Yagna identity management

#![allow(non_local_definitions)] // Due to Diesel macros.

pub mod cli;
pub mod service;

mod autoconf;
pub mod dao;
mod db;
mod id_key;
