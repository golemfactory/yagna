use ya_agreement_utils::agreement::flatten;

use serde_json::Value;

#[derive(thiserror::Error, Debug)]
pub enum FlattenError {
    #[error("JSON error: {0}")]
    SerdeJsonError(#[from] serde_json::error::Error),
}

pub fn flatten_properties(str_json_properties: &str) -> Result<Vec<String>, FlattenError> {
    flatten(serde_json::from_str(str_json_properties)?)
        .iter()
        .try_fold(vec![], |mut vec, (k, v)| {
            vec.push(format!("{}={}", k, serde_json::to_string(v)?));
            Ok(vec)
        })
}
