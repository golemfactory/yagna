use ya_agreement_utils::agreement::flatten;

use serde_json::Value;

#[derive(thiserror::Error, Debug)]
pub enum FlattenError {
    #[error("JSON error: {0}")]
    SerdeJsonError(#[from] serde_json::error::Error),
}

pub fn flatten_properties(str_json_properties: &str) -> Result<Vec<String>, FlattenError> {
    let json_properties: Value = serde_json::from_str(str_json_properties)?;
    let mapped = flatten(json_properties);
    let mut properties = vec![];
    for (k, v) in mapped.iter() {
        properties.push(format!("{}={}", k, serde_json::to_string(v)?))
    }

    Ok(properties)
}
