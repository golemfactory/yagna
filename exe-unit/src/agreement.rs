use ya_agreement_utils::agreement::{try_from_path, AgreementView, Error};

use crate::metrics::MemMetric;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Agreement {
    pub agreement: AgreementView,
    pub agreement_id: String,
    pub task_package: String,
    pub usage_vector: Vec<String>,
    pub usage_limits: HashMap<String, f64>,
}

impl Agreement {
    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        self.agreement.pointer(pointer)
    }
}

impl TryFrom<Value> for Agreement {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let agreement = AgreementView::try_from(value)?;

        let agreement_id = agreement.agreement_id.clone();
        let task_package = agreement
            .pointer_typed::<String>("/demand/properties/golem/srv/comp/wasm/task_package")?;
        let usage_vector =
            agreement.pointer_typed::<Vec<String>>("/offer/properties/golem/com/usage/vector")?;

        let limits = vec![(
            MemMetric::ID.to_owned(),
            agreement.pointer_typed::<f64>("/offer/properties/golem/inf/mem/gib")?,
        )]
        .into_iter()
        .collect();

        Ok(Agreement {
            agreement,
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
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn example_agreement() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("examples/agreement.json");
        Agreement::try_from(&path).unwrap();
    }
}
