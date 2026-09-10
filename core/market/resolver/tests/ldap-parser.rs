use ya_market_resolver::resolver::expression::{Expression, ResolveResult};
use ya_market_resolver::resolver::ldap_parser::parse;
use ya_market_resolver::resolver::properties::{PropertyRef, PropertyRefType, PropertySet};

fn property(name: &str) -> PropertyRef {
    PropertyRef::Value(name.to_owned(), PropertyRefType::Any)
}

#[test]
fn present() {
    assert_eq!(
        parse("(objectClass=*)"),
        Ok(Expression::Present(property("objectClass")))
    );
}

#[test]
fn comparisons() {
    assert_eq!(
        parse("(cn=Babs Jensen)"),
        Ok(Expression::Equals(property("cn"), "Babs Jensen".to_owned()))
    );
    assert_eq!(
        parse("(cn>Babs Jensen)"),
        Ok(Expression::Greater(
            property("cn"),
            "Babs Jensen".to_owned()
        ))
    );
    assert_eq!(
        parse("(cn>=Babs Jensen)"),
        Ok(Expression::GreaterEqual(
            property("cn"),
            "Babs Jensen".to_owned()
        ))
    );
    assert_eq!(
        parse("(cn<Babs Jensen)"),
        Ok(Expression::Less(property("cn"), "Babs Jensen".to_owned()))
    );
    assert_eq!(
        parse("(cn<=Babs Jensen)"),
        Ok(Expression::LessEqual(
            property("cn"),
            "Babs Jensen".to_owned()
        ))
    );
}

#[test]
fn not() {
    let expected = Expression::Not(Box::new(Expression::Equals(
        property("cn"),
        "Tim Howes".to_owned(),
    )));
    assert_eq!(parse("(!(cn=Tim Howes))"), Ok(expected.clone()));
    assert_eq!(parse("( !   (cn=Tim Howes))"), Ok(expected));
}

#[test]
fn and() {
    let expected = Expression::And(vec![
        Expression::Equals(property("a"), "b".to_owned()),
        Expression::Equals(property("b"), "c".to_owned()),
        Expression::Equals(property("c"), "d".to_owned()),
    ]);
    assert_eq!(parse("(&(a=b)(b=c)(c=d))"), Ok(expected.clone()));
    assert_eq!(parse("( &  (a=b)  (b=c)  (c=d) )"), Ok(expected));
}

#[test]
fn not_equal() {
    let expression = parse("(a<>b)").unwrap();

    let equal = vec!["a=\"b\"".to_owned()];
    let equal = PropertySet::from_flat_props(&equal).unwrap();
    assert!(matches!(
        expression.resolve(&equal),
        ResolveResult::False(_, _)
    ));

    let different = vec!["a=\"c\"".to_owned()];
    let different = PropertySet::from_flat_props(&different).unwrap();
    assert_eq!(expression.resolve(&different), ResolveResult::True);
}
