use std::ops::{Index, IndexMut};

pub struct ConstraintVec {
    values: Vec<Constraint>,
}

impl Index<&str> for ConstraintVec {
    type Output = ();
    fn index(&self, index: &str) -> &Self::Output {
        unimplemented!()
    }
}

impl IndexMut<&str> for ConstraintVec {
    fn index_mut(&mut self, index: &str) -> &mut Self::Output {
        unimplemented!()
    }
}

pub struct Constraint {
    key: serde_json::Value,
    operator: String,         /* TODO */
    value: serde_json::Value, /* TODO */
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraints() {
        // a.b.c = 1
        // d.e (no value)
        //
    }
}
