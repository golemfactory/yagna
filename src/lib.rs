#[macro_use]
extern crate nom;
extern crate asnom;
extern crate chrono;
extern crate uuid;
extern crate regex;
extern crate semver;

use std::error;
use std::fmt;
use std::collections::HashMap;
use uuid::Uuid;

pub mod provider;
pub mod requestor;
pub mod resolver;

// Id of Golem Node
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeId {}

#[derive(Debug, Default)]
pub struct Offer {
    pub offer_id : Uuid,
    pub provider_id : NodeId,

    // Properties (expressed in flat form, ie. as lines of text)
    pub properties : Vec<String>,

    // TODO REMOVE Explicit properties (with values)
    pub exp_properties : HashMap<String, String>,

    // Filter expression
    pub constraints : String,

    // TODO REMOVE Implicit properties (no values declared)
    pub imp_properties : Vec<String>,
}

#[derive(Debug, Default)]
pub struct Demand {
    pub demand_id : Uuid,
    pub requestor_id : NodeId,

    // Properties (expressed in flat form, ie. as lines of text)
    pub properties : Vec<String>,

    // TODO REMOVE Explicit properties (with values)
    pub exp_properties : HashMap<String, String>,

    // Filter expression
    pub constraints : String,

    // TODO REMOVE Implicit properties (no values declared)
    pub imp_properties : Vec<String>,
}

pub struct Agreement {
    pub agreement_id : Uuid,
}



// #region ScanError

#[derive(Debug, Clone, PartialEq)]
pub struct ScanError {

}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "scan failed")
    }
}

impl error::Error for ScanError {
    fn description(&self) -> &str {
        "scan failed"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region SubscribeError

#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeError {

}

impl fmt::Display for SubscribeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "subscription failed")
    }
}

impl error::Error for SubscribeError {
    fn description(&self) -> &str {
        "subscription failed"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region UnSubscribeError

#[derive(Debug, Clone, PartialEq)]
pub struct UnSubscribeError {

}

impl fmt::Display for UnSubscribeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "un-subscription failed")
    }
}

impl error::Error for UnSubscribeError {
    fn description(&self) -> &str {
        "un-subscription failed"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region CollectError

#[derive(Debug, Clone, PartialEq)]
pub struct CollectError {

}

impl fmt::Display for CollectError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "collect failed")
    }
}

impl error::Error for CollectError {
    fn description(&self) -> &str {
        "collect failed"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region ProposalError

#[derive(Debug, Clone, PartialEq)]
pub struct ProposalError {

}

impl fmt::Display for ProposalError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "post failed")
    }
}

impl error::Error for ProposalError {
    fn description(&self) -> &str {
        "post failed"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion

// #region AgreementError

#[derive(Debug, Clone, PartialEq)]
pub struct AgreementError {

}

impl fmt::Display for AgreementError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "agreement operation failed")
    }
}

impl error::Error for AgreementError {
    fn description(&self) -> &str {
        "agreement operation failed"
    }

    fn cause(&self) -> Option<&error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion