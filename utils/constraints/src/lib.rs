use serde::Serialize;
//use std::ops::{Index, IndexMut};

#[derive(Clone, Serialize)]
pub enum Constraints {
    Clause(ConstraintClause),
    Single(ConstraintExpr),
}

type ConstraintKey = serde_json::Value;
type ConstraintValue = serde_json::Value;

impl Constraints {
    pub fn try_from<T: Into<Constraints>>(
        op: ClauseOperator,
        v: Vec<T>,
    ) -> Result<Constraints, serde_json::error::Error> {
        Ok(Constraints::Clause(ConstraintClause {
            constraints: v.into_iter().map(|x| x.into()).collect(),
            operator: op,
        }))
    }
    /* TODO append */
}

//v: &[T],
// where for<'a> Constraints : From<&'a T> {

impl<T: Into<ConstraintExpr>> From<T> for Constraints {
    fn from(c: T) -> Self {
        Constraints::Single(c.into())
    }
}

impl<T: Into<ConstraintKey>, U: Into<ConstraintValue>> From<(T, ConstraintOperator, U)>
    for ConstraintExpr
{
    fn from((key, operator, value): (T, ConstraintOperator, U)) -> Self {
        ConstraintExpr::new(key.into(), operator, value.into())
    }
}

#[derive(Copy, Clone, Serialize)]
pub enum ClauseOperator {
    #[serde(rename = "&")]
    And,
    #[serde(rename = "|")]
    Or,
}

#[derive(Clone, Serialize)]
pub struct ConstraintClause {
    constraints: Vec<Constraints>,
    operator: ClauseOperator,
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
    #[serde(rename = "=")]
    Equal,
    #[serde(rename = "<>")]
    NotEqual,
    #[serde(rename = "<")]
    LessThan,
    #[serde(rename = ">")]
    GreaterThan,
}

impl ConstraintOperator {}

#[derive(Clone, Serialize)]
pub struct ConstraintExpr {
    key: ConstraintKey,
    operator: ConstraintOperator,
    value: ConstraintValue,
}

impl ConstraintExpr {
    fn new(
        key: ConstraintKey,
        operator: ConstraintOperator,
        value: ConstraintValue,
    ) -> ConstraintExpr {
        ConstraintExpr {
            key,
            operator,
            value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraints() {
        use serde_json::json;
        use ClauseOperator::*;
        use ConstraintOperator::*;
        let c = Constraints::try_from(
            And,
            vec![
                ("golem.inf.mem.gib", GreaterThan, 0.5).into(),
                ("golem.inf.storage.gib", GreaterThan, json!(1.0)).into(),
                ("golem.com.pricing.model", Equal, json!("linear")).into(),
                Constraints::try_from(
                    And,
                    vec![("golem.inf.storage.gib", GreaterThan, json!(1.0))],
                )
                .unwrap(),
            ],
        )
        .unwrap();
        println!("{}", serde_json::to_string_pretty(&c).unwrap());
    }
}
