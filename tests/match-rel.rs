extern crate market_api;

use market_api::{ Demand, Offer };
use market_api::resolver::*;
use market_api::resolver::match_rel::match_weak;

#[test]
fn match_weak_empty() {
    let demand = Demand::default();
    let offer = Offer::default();

    assert!(match match_weak(&demand, &offer) {
        Err(_) => true,
        _ => false
    });
}

#[test]
fn match_weak_simple_match() {
    let mut demand = Demand::default();
    demand.exp_properties.insert(String::from("d1"), String::from("v1"));
    demand.constraints = String::from("(o1=v2)");

    let mut offer = Offer::default();
    offer.exp_properties.insert(String::from("o1"), String::from("v2"));
    offer.constraints = String::from("(d1=v1)");

    assert_eq!(match_weak(&demand, &offer), Ok(ResolveResult::True));
}

#[test]
fn match_weak_simple_nonmatch() {
    let mut demand = Demand::default();
    demand.exp_properties.insert(String::from("d1"), String::from("v1"));
    demand.constraints = String::from("(o1=v2)");

    let mut offer = Offer::default();
    offer.exp_properties.insert(String::from("o1"), String::from("v2"));
    offer.constraints = String::from("(d1=v3)");

    assert_eq!(match_weak(&demand, &offer), Ok(ResolveResult::True));
}
