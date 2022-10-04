use ya_market_resolver::resolver::expression::*;
use ya_market_resolver::resolver::ldap_parser::parse;
use ya_market_resolver::resolver::properties::*;
use ya_market_resolver::resolver::*;
use ya_market_resolver::*;

#[test]
fn prepare_offer_error_for_empty() {
    let demand = Demand::default();

    assert!(PreparedDemand::from(&demand).is_err());
}

#[test]
fn prepare_demand_error_for_empty() {
    let offer = Offer::default();

    assert!(PreparedOffer::from(&offer).is_err());
}

#[test]
fn build_expression_empty() {
    let f = "()";

    let expression = Expression::Empty(true);

    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn build_expression_present() {
    let f = "(objectClass=*)";

    let expression = Expression::Present(PropertyRef::Value(
        String::from("objectClass"),
        PropertyRefType::Any,
    ));

    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn build_expression_equals() {
    let f = "(cn=Babs Jensen)";

    let expression = Expression::Equals(
        PropertyRef::Value(String::from("cn"), PropertyRefType::Any),
        String::from("Babs Jensen"),
    );

    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn build_expression_not() {
    let f = "(!(cn=Tim Howes))";

    let expression = Expression::Not(Box::new(Expression::Equals(
        PropertyRef::Value(String::from("cn"), PropertyRefType::Any),
        String::from("Tim Howes"),
    )));

    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}

#[test]
fn build_expression_and() {
    let f = "(&(a=b)(b=c)(c=d))";

    let expression = Expression::And(vec![
        Box::new(Expression::Equals(
            PropertyRef::Value(String::from("a"), PropertyRefType::Any),
            String::from("b"),
        )),
        Box::new(Expression::Equals(
            PropertyRef::Value(String::from("b"), PropertyRefType::Any),
            String::from("c"),
        )),
        Box::new(Expression::Equals(
            PropertyRef::Value(String::from("c"), PropertyRefType::Any),
            String::from("d"),
        )),
    ]);

    assert_eq!(build_expression(&parse(f).unwrap()), Ok(expression));
}
