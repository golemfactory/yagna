use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum NegotiationApiInitError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ProposalError {}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum AgreementError {}
