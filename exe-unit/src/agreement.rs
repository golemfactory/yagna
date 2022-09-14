use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;
use std::str::FromStr;

use serde_json::Value;

use crate::metrics::{MemMetric, StorageMetric};
use ya_agreement_utils::agreement::{try_from_path, AgreementView, Error};
use ya_client_model::NodeId;

#[derive(Clone, Debug)]
pub struct Agreement {
    pub inner: AgreementView,
    pub requestor_id: NodeId,
    pub task_package: Option<String>,
    pub usage_vector: Vec<String>,
    pub usage_limits: HashMap<String, f64>,
    pub infrastructure: HashMap<String, f64>,
}

impl Agreement {
    #[inline]
    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        self.inner.pointer(pointer)
    }
}

impl TryFrom<Value> for Agreement {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let agreement = AgreementView::try_from(value)?;
        let task_package = agreement
            .pointer_typed::<String>("/demand/properties/golem/srv/comp/task_package")
            .ok();
        let usage_vector =
            agreement.pointer_typed::<Vec<String>>("/offer/properties/golem/com/usage/vector")?;
        let infra = agreement.properties::<f64>("/offer/properties/golem/inf")?;
        let requestor_id = agreement.pointer_typed::<String>("/demand/requestorId")?;
        let requestor_id = NodeId::from_str(requestor_id.as_str())
            .map_err(|_| Error::InvalidValue("requestorId".to_string()))?;

        let limits = vec![
            (MemMetric::ID, MemMetric::INF),
            (StorageMetric::ID, StorageMetric::INF),
        ]
        .into_iter()
        .filter_map(|(id, inf)| infra.get(inf).map(|v| (id.to_string(), *v)))
        .collect();

        Ok(Agreement {
            inner: agreement,
            requestor_id,
            task_package,
            usage_vector,
            usage_limits: limits,
            infrastructure: infra,
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
