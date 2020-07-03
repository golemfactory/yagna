use semver::Version;

use ya_market_resolver::resolver::properties::*;

// #region Version type

#[test]
fn equals_for_version_simple_true() {
    let prop_value = PropertyValue::Version(Version::parse("0.5.0").unwrap());

    assert_eq!(prop_value.equals("0.5.0"), true);
}

#[test]
fn equals_for_version_simple_false() {
    let prop_value = PropertyValue::Version(Version::parse("0.5.0").unwrap());

    assert_eq!(prop_value.equals("0.6.1"), false);
}

#[test]
fn less_for_version_simple_true() {
    let prop_value = PropertyValue::Version(Version::parse("0.5.0").unwrap());

    assert_eq!(prop_value.less("0.6.0"), true);
}

#[test]
fn less_equal_for_version_simple_true() {
    let prop_value = PropertyValue::Version(Version::parse("0.5.0").unwrap());

    assert_eq!(prop_value.less_equal("0.5.0"), true);
}

#[test]
fn greater_for_version_simple_true() {
    let prop_value = PropertyValue::Version(Version::parse("0.5.0").unwrap());

    assert_eq!(prop_value.greater("0.4.0"), true);
}

#[test]
fn greater_equal_for_version_simple_true() {
    let prop_value = PropertyValue::Version(Version::parse("0.5.0").unwrap());

    assert_eq!(prop_value.greater_equal("0.5.0"), true);
}

// #endregion
