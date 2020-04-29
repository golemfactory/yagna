use serde::export::Formatter;
use std::fmt;
use std::ops::{Index, IndexMut};

#[derive(Clone)]
pub struct Constraints {
    constraints: Vec<ConstraintExpr>,
    operator: ClauseOperator,
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

impl Index<&str> for Constraints {
    type Output = ();
    fn index(&self, _index: &str) -> &Self::Output {
        unimplemented!()
    }
}

impl IndexMut<&str> for Constraints {
    fn index_mut(&mut self, _index: &str) -> &mut Self::Output {
        unimplemented!()
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

impl From<&str> for ConstraintKey {
    fn from(key: &str) -> Self {
        ConstraintKey::new(serde_json::Value::String(key.to_string()))
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
macro_rules! constraints_and [
    ( $($key:tt $( $op:tt $val:tt )? ),* ) => {{
        Constraints::new_clause::<ConstraintExpr>(ClauseOperator::And, vec![
            $({
                let c = ConstraintKey::new($key);
                let mut e: Option<ConstraintExpr> = Option::None;
                $(
                    let val = ConstraintValue::new($val);
                    let c2 = c.clone();
                    e = Some(match stringify!($op) {
                        ">" => c2.greater_than(val),
                        "<" => c2.less_than(val),
                        "=" => c2.equal_to(val),
                        "!=" => c2.not_equal_to(val),
                        e => panic!("expected >, <, =, !=, got {}", e)
                    });
                )?
                e.unwrap_or(c.into())
            },
            )*
        ])
    }};
];
