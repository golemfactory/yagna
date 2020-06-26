use thiserror::Error;
use serde::{Deserialize, Serialize};


#[derive(Error, Debug, Serialize, Deserialize)]
pub enum NegotiationApiInitError {

}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum ProposalError {

}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum AgreementError {

}

