use serde::export::Formatter;
use std::fmt;

#[derive(Clone)]
pub struct Constraints {
    pub constraints: Vec<ConstraintExpr>,
    pub operator: ClauseOperator,
}

impl Constraints {
    pub fn new_clause<T: Into<ConstraintExpr>>(op: ClauseOperator, v: Vec<T>) -> Constraints {
        Constraints {
            constraints: v.into_iter().map(|x| x.into()).collect(),
            operator: op,
        }
    }
    pub fn new_single<T: Into<ConstraintExpr>>(el: T) -> Constraints {
        Constraints {
            constraints: vec![el.into()],
            operator: ClauseOperator::And,
        }
    }
    pub fn or(self, c: Constraints) -> Constraints {
        self.joined_with(c, ClauseOperator::Or)
    }
    pub fn and(self, c: Constraints) -> Constraints {
        self.joined_with(c, ClauseOperator::And)
    }
    fn joined_with(self, c: Constraints, operator: ClauseOperator) -> Constraints {
        if c.operator == operator && self.operator == operator {
            Constraints {
                constraints: [&self.constraints[..], &c.constraints[..]].concat(),
                operator: self.operator,
            }
        } else {
            Constraints::new_clause(operator, vec![self, c])
        }
    }
    pub fn without<T: Into<ConstraintKey>>(self, removed_key: T) -> Constraints {
        let op = self.operator;
        let del_key = removed_key.into();
        Constraints {
            constraints: self
                .into_iter()
                .filter(|c| match c {
                    ConstraintExpr::KeyValue { key, .. } => *key != del_key,
                    _ => true,
                })
                .collect(),
            operator: op,
        }
    }
    pub fn filter_by_key<T: Into<ConstraintKey>>(&self, get_key: T) -> Option<Constraints> {
        let k = get_key.into();
        let v: Vec<_> = self
            .constraints
            .iter()
            .cloned()
            .filter(|e| match e {
                ConstraintExpr::KeyValue { key, .. } => *key == k,
                ConstraintExpr::Constraints(_) => false,
            })
            .collect();
        match v.len() {
            0 => None,
            1 => Some(Self::new_single(v[0].clone())),
            _ => Some(Self::new_clause(self.operator, v)),
        }
    }
}

impl fmt::Display for Constraints {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.constraints.len() {
            0 => Ok(()),
            1 => write!(f, "{}", self.constraints[0]),
            _ => {
                write!(f, "({}\n", self.operator.to_string())?;
                for el in &self.constraints {
                    write!(f, "  {}\n", el.to_string().replace("\n", "\n  "))?;
                }
                write!(f, ")")
            }
        }
    }
}

impl std::iter::IntoIterator for Constraints {
    type Item = ConstraintExpr;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.constraints.into_iter()
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum ClauseOperator {
    And,
    Or,
}

impl fmt::Display for ClauseOperator {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ClauseOperator::And => "&",
                ClauseOperator::Or => "|",
            }
        )
    }
}

#[derive(Copy, Clone)]
pub enum ConstraintOperator {
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
}

impl fmt::Display for ConstraintOperator {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ConstraintOperator::Equal => "=",
                ConstraintOperator::NotEqual => "<>",
                ConstraintOperator::LessThan => "<",
                ConstraintOperator::GreaterThan => ">",
            }
        )
    }
}

#[derive(Clone, PartialEq)]
pub struct ConstraintKey(serde_json::Value);

impl ConstraintKey {
    pub fn new<T: Into<serde_json::Value>>(v: T) -> Self {
        ConstraintKey(v.into())
    }
}

impl<T: AsRef<str>> From<T> for ConstraintKey {
    fn from(key: T) -> Self {
        ConstraintKey::new(serde_json::Value::String(key.as_ref().to_string()))
    }
}

pub type ConstraintValue = ConstraintKey;

/* expression, e.g. key > value */
#[derive(Clone)]
pub enum ConstraintExpr {
    KeyValue {
        /* ops_values length is 0 or 1 now, but it's ready for expressions like k: > v1, < v2 */
        key: ConstraintKey,
        ops_values: Vec<(ConstraintOperator, ConstraintValue)>,
    },
    Constraints(Constraints),
}

impl From<ConstraintKey> for ConstraintExpr {
    fn from(key: ConstraintKey) -> Self {
        ConstraintExpr::KeyValue {
            key,
            ops_values: vec![],
        }
    }
}

impl From<Constraints> for ConstraintExpr {
    fn from(key: Constraints) -> Self {
        ConstraintExpr::Constraints(key)
    }
}

impl fmt::Display for ConstraintExpr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ConstraintExpr::KeyValue { key, ops_values } => {
                if ops_values.len() == 0 {
                    write!(f, "({})", key.0.as_str().unwrap_or(&key.0.to_string()))
                } else {
                    for (op, val) in ops_values {
                        write!(f, "({}", key.0.as_str().unwrap_or(&key.0.to_string()))?;
                        write!(f, "{}", op.to_string())?;
                        write!(f, "{}", val.0.as_str().unwrap_or(&val.0.to_string()))?;
                        write!(f, ")")?
                    }
                    Ok(())
                }
            }
            ConstraintExpr::Constraints(c) => write!(f, "{}", c.to_string()),
        }
    }
}

impl ConstraintKey {
    fn with_operator_value(
        self,
        operator: ConstraintOperator,
        value: ConstraintValue,
    ) -> ConstraintExpr {
        ConstraintExpr::KeyValue {
            key: self,
            ops_values: vec![(operator, value)],
        }
    }
    pub fn greater_than(self, value: ConstraintValue) -> ConstraintExpr {
        self.with_operator_value(ConstraintOperator::GreaterThan, value)
    }
    pub fn less_than(self, value: ConstraintValue) -> ConstraintExpr {
        self.with_operator_value(ConstraintOperator::LessThan, value)
    }
    pub fn equal_to(self, value: ConstraintValue) -> ConstraintExpr {
        self.with_operator_value(ConstraintOperator::Equal, value)
    }
    pub fn not_equal_to(self, value: ConstraintValue) -> ConstraintExpr {
        self.with_operator_value(ConstraintOperator::NotEqual, value)
    }
}

#[macro_export]
macro_rules! constraints [
    () => {};
    ($key:tt == $value:expr $(,)*) => {{ Constraints::new_single(ConstraintKey::new($key).equal_to(ConstraintKey::new($value))) }};
    ($key:tt == $value:expr , $($r:tt)*) => {{ Constraints::new_single(ConstraintKey::new($key).equal_to(ConstraintKey::new($value))).and(constraints!( $($r)* )) }};
    ($key:tt != $value:expr $(,)*) => {{ Constraints::new_single(ConstraintKey::new($key).not_equal_to(ConstraintKey::new($value))) }};
    ($key:tt != $value:expr , $($r:tt)*) => {{ Constraints::new_single(ConstraintKey::new($key).not_equal_to(ConstraintKey::new($value))).and(constraints!( $($r)* )) }};
    ($key:tt < $value:expr $(,)*) => {{ Constraints::new_single(ConstraintKey::new($key).less_than(ConstraintKey::new($value))) }};
    ($key:tt < $value:expr , $($r:tt)*) => {{ Constraints::new_single(ConstraintKey::new($key).less_than(ConstraintKey::new($value))).and(constraints!( $($r)* )) }};
    ($key:tt > $value:expr $(,)*) => {{ Constraints::new_single(ConstraintKey::new($key).greater_than(ConstraintKey::new($value))) }};
    ($key:tt > $value:expr , $($r:tt)*) => {{ Constraints::new_single(ConstraintKey::new($key).greater_than(ConstraintKey::new($value))).and(constraints!( $($r)* )) }};
    ($key:tt $(,)*) => {{ Constraints::new_single(ConstraintKey::new($key)) }};
    ($key:tt , $($r:tt)*) => {{ Constraints::new_single(ConstraintKey::new($key)).and(constraints!( $($r)* )) }};
    ($t:expr $(,)*) => { $t };
    ($t:expr , $($r:tt)*) => {
        $t.and(constraints!( $($r)* ))
    };
];
