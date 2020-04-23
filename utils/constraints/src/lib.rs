use serde::export::Formatter;
use serde::Serialize;
use std::fmt;
//use std::ops::{Index, IndexMut};

#[derive(Clone, Serialize)]
pub enum Constraints {
    Single(ConstraintExpr),
    Clause(ConstraintClause),
}

type ConstraintKey = serde_json::Value;
type ConstraintValue = serde_json::Value;

impl Constraints {
    pub fn new_clause<T: Into<Constraints>>(op: ClauseOperator, v: Vec<T>) -> Constraints {
        Constraints::Clause(ConstraintClause {
            constraints: v.into_iter().map(|x| x.into()).collect(),
            operator: op,
        })
    }
    pub fn new_single<T: Into<ConstraintExpr>>(el: T) -> Constraints {
        Constraints::Single(el.into())
    }
}

//v: &[T],; where for<'a> Constraints : From<&'a T> {

impl<T: Into<ConstraintExpr>> From<T> for Constraints {
    fn from(c: T) -> Self {
        Constraints::Single(c.into())
    }
}

impl fmt::Display for Constraints {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Constraints::Single(expr) => expr.to_string(),
                Constraints::Clause(clause) => clause.to_string(),
            }
        )
    }
}

impl<T: Into<ConstraintKey>, U: Into<ConstraintValue>> From<(T, ConstraintOperator, U)>
    for ConstraintExpr
{
    fn from((key, operator, value): (T, ConstraintOperator, U)) -> Self {
        ConstraintExpr::with_key_op_value(key.into(), operator, value.into())
    }
}

/*impl<T: Into<ConstraintKey>> From<T> for ConstraintExpr
{
    fn from(key: T) -> Self {
        ConstraintExpr::with_key(key.into())
    }
}*/

#[derive(Copy, Clone, Serialize)]
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

#[derive(Clone, Serialize)]
pub struct ConstraintClause {
    constraints: Vec<Constraints>,
    operator: ClauseOperator,
}

impl fmt::Display for ConstraintClause {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "({}\n", self.operator.to_string())?;
        for el in &self.constraints {
            write!(f, "  {}\n", el.to_string().replace("\n", "\n  "))?;
        }
        write!(f, ")")
    }
}

/*
impl Index<&str> for ConstraintClause {
    type Output = ();
    fn index(&self, index: &str) -> &Self::Output {
        unimplemented!()
    }
}

impl IndexMut<&str> for ConstraintClause {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        unimplemented!()
    }
}*/

#[derive(Copy, Clone, Serialize)]
pub enum ConstraintOperator {
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
}

impl ConstraintOperator {}

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

#[derive(Clone, Serialize)]
pub struct ConstraintExpr {
    key: ConstraintKey,
    operator: Option<ConstraintOperator>,
    value: Option<ConstraintValue>,
}

impl ConstraintExpr {
    pub fn with_key(key: ConstraintKey) -> ConstraintExpr {
        ConstraintExpr {
            key,
            operator: None,
            value: None,
        }
    }
    pub fn with_key_op_value(
        key: ConstraintKey,
        operator: ConstraintOperator,
        value: ConstraintValue,
    ) -> ConstraintExpr {
        ConstraintExpr {
            key,
            operator: Some(operator),
            value: Some(value),
        }
    }
}

impl fmt::Display for ConstraintExpr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "({}", self.key.as_str().unwrap_or(&self.key.to_string()))?;
        if let Some(operator) = &self.operator {
            write!(f, "{}", operator.to_string())?;
        }
        if let Some(value) = &self.value {
            write!(f, "{}", value.as_str().unwrap_or(&value.to_string()))?
        }
        write!(f, ")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraints() {
        use ClauseOperator::*;
        use ConstraintOperator::*;
        let c = Constraints::new_clause(
            And,
            vec![
                ("golem.inf.mem.gib", GreaterThan, 0.5).into(),
                ("golem.inf.storage.gib", GreaterThan, 1.0).into(),
                ("golem.com.pricing.model", Equal, "linear").into(),
                Constraints::new_clause(
                    Or,
                    vec![
                        ("golem.inf.storage.gib", GreaterThan, 1.0),
                        ("golem.inf.storage.gib", GreaterThan, 2.0),
                    ],
                ),
            ],
        );
        println!("{}", c.to_string());
    }
}
