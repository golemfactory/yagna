use actix_web::ResponseError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NegotiationError {}

impl ResponseError for NegotiationError {}

#[derive(Error, Debug)]
pub enum NegotiationInitError {}
