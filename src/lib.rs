#[macro_use]
extern crate nom;
extern crate asnom;
extern crate chrono;
extern crate uuid;
extern crate regex;
extern crate semver;
extern crate decimal;

use std::error;
use std::fmt;
use std::collections::HashMap;
use uuid::Uuid;

use resolver::ldap_parser::parse;
use resolver::expression::*;
use resolver::properties::PropertySet;


pub mod provider;
pub mod requestor;
pub mod resolver;

pub use resolver::matching::{ MatchResult, match_weak };
pub use resolver::prepare::{ PreparedOffer, PreparedDemand };

// #region Externally visible functions
#[repr(C)]
pub struct StringRef {
    bytes: *const u8,
    length: u32
}

fn unpack_string_ref_array<'a>(str_ref_arr : *const StringRef, count: u32) -> Vec<String> {
    let array_slice = unsafe { std::slice::from_raw_parts(str_ref_arr, count as usize) };
    let mut result = vec![];
    for item in array_slice {
        result.push(String::from(unpack_string_ref(&item)));
    };

    result
}

fn unpack_string_ref<'a>(str_ref : &StringRef) -> &str {
    let slice = unsafe { 
        std::slice::from_raw_parts(str_ref.bytes, str_ref.length as usize) 
        };
    let str_result = std::str::from_utf8(slice);
    match str_result {
        Ok(str_content) => {
            str_content
            },
        Err(error) => {
            println!("{:?}", error);
            panic!(error);
        }
    }
}


#[no_mangle]
pub extern fn match_demand_offer(demand_props: *const StringRef, demand_props_count: u32,
                                 demand_constraints: StringRef, 
                                 offer_props: *const StringRef, offer_props_count: u32,
                                 offer_constraints: StringRef) -> i32 {
    let mut demand = Demand::default();
    demand.properties = unpack_string_ref_array(demand_props, demand_props_count);
    demand.constraints = String::from(unpack_string_ref(&demand_constraints));

    let mut offer = Offer::default();
    offer.properties = unpack_string_ref_array(offer_props, offer_props_count);
    offer.constraints = String::from(unpack_string_ref(&offer_constraints));

    match match_weak(&PreparedDemand::from(&demand).unwrap(), &PreparedOffer::from(&offer).unwrap()){
        Ok(match_result) => {
            match match_result {
                MatchResult::True => 1,
                MatchResult::False(_,_) => 0,
                MatchResult::Undefined(_,_) => 2,
                MatchResult::Err(_) => -1
            }
        },
        Err(_) => {
            -1
        }
    }

}

#[no_mangle]
pub extern fn resolve_expression(expression: StringRef, props: *const StringRef, props_count: u32) -> i32 {
    let expr = unpack_string_ref(&expression);
    let expr_tree = match parse(expr) {
        Ok(tag) => tag,
        Err(_) => { return -1; }
    };

    let expression =
        match build_expression(&expr_tree)
        {
            Ok(express) => express,
            Err(_) => { return -1; }
        };

    let properties = unpack_string_ref_array(props, props_count);

    let property_set = PropertySet::from_flat_props(&properties);

    match expression.resolve(&property_set) {
        ResolveResult::True => 1,
        ResolveResult::False(_, _) => 0,
        ResolveResult::Undefined(_, _) => 2,
        _ => -1
    }

}


// #endregion


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

    fn cause(&self) -> Option<&dyn error::Error> {
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

    fn cause(&self) -> Option<&dyn error::Error> {
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

    fn cause(&self) -> Option<&dyn error::Error> {
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

    fn cause(&self) -> Option<&dyn error::Error> {
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

    fn cause(&self) -> Option<&dyn error::Error> {
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

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

// #endregion