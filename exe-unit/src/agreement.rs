use ya_agreement_utils::agreement::{try_from_path, Error, TypedPointer, TypedArrayPointer};

use crate::metrics::MemMetric;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;


#[derive(Clone, Debug)]
pub struct Agreement {
    pub json: Value,
    pub agreement_id: String,
    pub task_package: String,
    pub usage_vector: Vec<String>,
    pub usage_limits: HashMap<String, f64>,
}

impl Agreement {
    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        self.json.pointer(pointer)
    }
}

impl TryFrom<Value> for Agreement {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let agreement_id = value
            .pointer("/agreementId")
            .as_typed(Value::as_str)?
            .to_owned();
        let task_package = value
            .pointer("/demand/properties/golem/srv/comp/wasm/task_package")
            .as_typed(Value::as_str)?
            .to_owned();
        let usage_vector = value
            .pointer("/offer/properties/golem/com/usage/vector")
            .as_typed_array(|v| v.as_str().map(|s| s.to_owned()))?;

        let limits = vec![(
            MemMetric::ID.to_owned(),
            value
                .pointer("/offer/properties/golem/inf/mem/gib")
                .as_typed(Value::as_f64)?,
        )]
            .into_iter()
            .collect();

        Ok(Agreement {
            json: value,
            agreement_id,
            task_package,
            usage_vector,
            usage_limits: limits,
        })
    }
}

impl TryFrom<&PathBuf> for Agreement {
    type Error = Error;

    fn try_from(path: &PathBuf) -> Result<Self, Self::Error> {
        Self::try_from(try_from_path(path)?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use super::*;

    #[test]
    fn example_agreement() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("examples/agreement.json");
        Agreement::try_from(&path).unwrap();
    }
}