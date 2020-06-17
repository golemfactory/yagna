use thiserror::Error;

#[derive(Error, Debug)]
pub enum NegotiationError {}

#[derive(Error, Debug)]
pub enum NegotiationInitError {}

#[derive(Error, Debug)]
pub enum QueryEventsError {}

#[derive(Error, Debug)]
pub enum ProposalError {}
