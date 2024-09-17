use bigdecimal::BigDecimal;

fn big_decimal_normalize(bd: &BigDecimal, decimal_places: u8) -> String {
    if decimal_places == 0 {
        panic!("Decimal places must be greater than 0");
    }
    let mut s = bd.to_string();
    let find_idx = s.find('.');
    if let Some(dot_idx) = find_idx {
        if s.len() > dot_idx + 1 + decimal_places as usize {
            s = s[0..(dot_idx + 1 + decimal_places as usize)].to_string();
        } else {
            for _ in 0..(dot_idx + 1 + decimal_places as usize - s.len()) {
                s += "0";
            }
        }
    } else {
        s += ".";
        for _ in 0..decimal_places {
            s += "0";
        }
    }
    s
}

pub fn big_decimal_normalize_18(bd: &BigDecimal) -> String {
    big_decimal_normalize(bd, 18)
}

// some tests

#[test]
fn test_normie() {
    use std::str::FromStr;
    assert_eq!(big_decimal_normalize_18(&BigDecimal::from_str("1.12345").unwrap()), "1.123450000000000000");
    assert_eq!(big_decimal_normalize_18(&BigDecimal::from_str("1.123453333333333333333331").unwrap()), "1.123453333333333333");
    assert_eq!(big_decimal_normalize_18(&BigDecimal::from_str("555").unwrap()), "555.000000000000000000");
    assert_eq!(big_decimal_normalize_18(&BigDecimal::from_str("0").unwrap()), "0.000000000000000000");
}
