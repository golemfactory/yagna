use ya_market_resolver::resolver::error::*;
use ya_market_resolver::resolver::matching::*;
use ya_market_resolver::resolver::properties::*;
use ya_market_resolver::resolver::*;
use ya_market_resolver::{Demand, Offer};

#[test]
fn match_weak_simple_match() {
    let mut demand = Demand::default();
    demand.properties.push(String::from("d1=\"v1\""));
    demand.constraints = String::from("(o1=v2)");

    let mut offer = Offer::default();
    offer.properties.push(String::from("o1=\"v2\""));
    offer.constraints = String::from("(d1=v1)");

    assert_eq!(
        match_weak(
            &PreparedDemand::from(&demand).unwrap(),
            &PreparedOffer::from(&offer).unwrap()
        ),
        Ok(MatchResult::True)
    );
}

#[test]
fn match_simple_error() {
    let mut demand = Demand::default();
    demand.properties.push(String::from("d5=\"v1\""));
    demand.constraints = String::from("werwer(werwerewro1=v2)");

    let mut offer = Offer::default();
    offer.properties.push(String::from("o1=\"v2\""));
    offer.constraints = String::from("(d1=v1)");

    let prep_demand_result = PreparedDemand::from(&demand);

    match prep_demand_result {
        Ok(_) => panic!("Demand content error was not caught!"),
        Err(prep_error) => assert_eq!(
            prep_error,
            PrepareError::new("Error parsing Demand constraints: Parsing error: Alternative")
        ),
    }
}

#[test]
fn match_weak_simple_no_match() {
    let mut demand = Demand::default();
    demand.properties.push(String::from("d1=\"v1\""));
    demand.constraints = String::from("(o1=v2)");

    let mut offer = Offer::default();
    offer.properties.push(String::from("o1=\"v2\""));
    offer.constraints = String::from("(d1=v3)");

    assert_eq!(
        match_weak(
            &PreparedDemand::from(&demand).unwrap(),
            &PreparedOffer::from(&offer).unwrap()
        ),
        Ok(MatchResult::False(vec![], vec!()))
    );
}

#[test]
fn match_weak_simple_undefined() {
    let mut demand = Demand::default();
    demand.properties.push(String::from("d1=\"v1\""));
    demand.constraints = String::from("(o3=v2)"); // unresolved property

    let mut offer = Offer::default();
    offer.properties.push(String::from("o1=\"v2\""));
    offer.constraints = String::from("(d1=v3)");

    assert_eq!(
        match_weak(
            &PreparedDemand::from(&demand).unwrap(),
            &PreparedOffer::from(&offer).unwrap()
        ),
        Ok(MatchResult::Undefined(
            (
                vec![&PropertyRef::Value(
                    String::from("o3"),
                    PropertyRefType::Any
                )],
                Expression::Equals(
                    PropertyRef::Value(String::from("o3"), PropertyRefType::Any),
                    String::from("v2")
                )
            ),
            (vec![], Expression::Empty(false))
        ))
    );
}

#[test]
fn match_weak_dynamic_property_match() {
    let mut demand = Demand::default();
    demand.properties.push(String::from("d1=\"v1\""));
    demand.constraints = String::from("(o1=*)");

    let mut offer = Offer::default();
    offer.properties.push(String::from("o1"));
    offer.constraints = String::from("(d1=v1)");

    assert_eq!(
        match_weak(
            &PreparedDemand::from(&demand).unwrap(),
            &PreparedOffer::from(&offer).unwrap()
        ),
        Ok(MatchResult::True)
    );
}

#[test]
fn match_weak_dynamic_property_no_match() {
    let mut demand = Demand::default();
    demand.properties.push(String::from("d1=\"v1\""));
    demand.constraints = String::from("(o1dblah=*)");

    let mut offer = Offer::default();
    offer.properties.push(String::from("o1"));
    offer.constraints = String::from("(d1=v1)");

    assert_eq!(
        match_weak(
            &PreparedDemand::from(&demand).unwrap(),
            &PreparedOffer::from(&offer).unwrap()
        ),
        Ok(MatchResult::False(
            vec![&PropertyRef::Value(
                String::from("o1dblah"),
                PropertyRefType::Any
            )],
            vec![]
        ))
    );
}

#[ignore]
#[test]
fn match_weak_dynamic_property_wildcard_match() {
    let mut demand = Demand::default();
    demand.properties.push(String::from("d1=\"v1\""));
    demand.constraints = String::from("(o1{dblah}=true)");

    let mut offer = Offer::default();
    offer.properties.push(String::from("o1*"));
    offer.constraints = String::from("(d1=v1)");

    assert_eq!(
        match_weak(
            &PreparedDemand::from(&demand).unwrap(),
            &PreparedOffer::from(&offer).unwrap()
        ),
        Ok(MatchResult::True)
    );
}

#[test]
fn match_weak_simple_aspect_match() {
    let mut demand = Demand::default();
    demand.properties.push(String::from(r#"d1="v1""#));
    demand.constraints = String::from("(&(o1=v2)(o1[aspect]=dblah))");

    let mut offer = Offer::default();
    offer.properties.push(String::from("o1=\"v2\""));
    offer.constraints = String::from("(d1=v1)");

    let prepared_demand = PreparedDemand::from(&demand).unwrap();
    let mut prepared_offer = PreparedOffer::from(&offer).unwrap();

    // Inject aspect here (note this seems very inefficient - worth review)
    prepared_offer
        .properties
        .set_property_aspect("o1", "aspect", "dblah");

    assert_eq!(
        match_weak(&prepared_demand, &prepared_offer),
        Ok(MatchResult::True)
    );
}
