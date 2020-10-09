use serde_json::{json, Map, Value};

#[derive(thiserror::Error, Debug)]
#[error("Flattened JSON should be object, but found: {0}")]
pub struct JsonObjectExpected(String);

#[derive(thiserror::Error, Debug)]
pub enum FlattenError {
    #[error("JSON error: {0}")]
    SerdeJsonError(#[from] serde_json::error::Error),
    #[error(transparent)]
    JsonObjectExpected(#[from] JsonObjectExpected),
}

pub fn flatten_properties(str_json_properties: &str) -> Result<Vec<String>, FlattenError> {
    let json_properties: Value = serde_json::from_str(str_json_properties)?;

    let mut properties = vec![];
    for (k, v) in flatten_json(&json_properties)?.as_object().unwrap().iter() {
        properties.push(format!("{}={}", k, serde_json::to_string(v).unwrap()))
    }

    Ok(properties)
}

pub fn flatten_json(json: &Value) -> Result<Value, JsonObjectExpected> {
    let mut flat_json: Value = json!({});
    if let Some(obj) = json.as_object() {
        flatten_object(obj, &None, &mut flat_json);
    } else {
        return Err(JsonObjectExpected(json.to_string()));
    }

    Ok(flat_json)
}

fn flatten_object(
    obj_input: &Map<String, Value>,
    key_prefix: &Option<String>,
    flat_output: &mut Value,
) {
    for (k, v) in obj_input.iter() {
        let new_k = match key_prefix {
            Some(ref prefix) => [prefix, ".", k].join(""),
            None => k.clone(),
        };
        if let Some(obj) = v.as_object() {
            flatten_object(obj, &Some(new_k), flat_output);
        } else if let Some(value) = flat_output.as_object_mut() {
            value.insert(new_k, v.clone());
        }
    }
}
