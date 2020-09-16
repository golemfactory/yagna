use ya_client_model::market::Agreement;

use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;

pub const PROPERTY_TAG: &str = "@tag";
const DEFAULT_FORMAT: &str = "json";

// TODO: Consider different structure:
//  - 2 fields for parsed properties (demand, offer)
//  - other fields for agreement remain typed.
// TODO: Move to ya-client to make it available for third party developers.
#[derive(Clone, Debug)]
pub struct AgreementView {
    pub json: Value,
    pub agreement_id: String,
}

impl AgreementView {
    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        self.json.pointer(pointer)
    }

    pub fn pointer_typed<'a, T: Deserialize<'a>>(&self, pointer: &str) -> Result<T, Error> {
        let value = self
            .json
            .pointer(pointer)
            .ok_or(Error::NoKey(pointer.to_string()))?
            .clone();
        Ok(<T as Deserialize>::deserialize(value)
            .map_err(|error| Error::UnexpectedType(pointer.to_string(), error))?)
    }

    pub fn properties<'a, T: Deserialize<'a>>(
        &self,
        pointer: &str,
    ) -> Result<HashMap<String, T>, Error> {
        let value = self
            .pointer(pointer)
            .ok_or(Error::NoKey(pointer.to_string()))?;

        let mut map = Map::new();
        properties(String::new(), &mut map, value.clone());
        let map = map
            .into_iter()
            .filter_map(|(k, v)| match <T as Deserialize>::deserialize(v) {
                Ok(v) => Some((k, v)),
                Err(_) => None,
            })
            .collect();
        Ok(map)
    }
}

impl TryFrom<Value> for AgreementView {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let agreement_id = value
            .pointer("/agreementId")
            .as_typed(Value::as_str)?
            .to_owned();

        Ok(AgreementView {
            json: value,
            agreement_id,
        })
    }
}

impl TryFrom<&PathBuf> for AgreementView {
    type Error = Error;

    fn try_from(path: &PathBuf) -> Result<Self, Self::Error> {
        Self::try_from(try_from_path(path)?)
    }
}

impl TryFrom<&Agreement> for AgreementView {
    type Error = Error;

    fn try_from(agreement: &Agreement) -> Result<Self, Self::Error> {
        Self::try_from(expand(serde_json::to_value(agreement)?))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Invalid YAML: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("Invalid value: {0}")]
    InvalidValue(String),
    #[error("Key '{0}' doesn't exist")]
    NoKey(String),
    #[error("Key '{0}' has invalid type. Error: {1}")]
    UnexpectedType(String, serde_json::Error),
}

pub trait TypedPointer {
    fn as_typed<'v, F, T>(&'v self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&'v Value) -> Option<T>;
}

impl TypedPointer for Option<&Value> {
    fn as_typed<'v, F, T>(&'v self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&'v Value) -> Option<T>,
    {
        self.map(f)
            .flatten()
            .ok_or(Error::InvalidValue(format!("{:?}", self)))
    }
}

pub trait TypedArrayPointer {
    fn as_typed_array<'v, F, T>(&'v self, f: F) -> Result<Vec<T>, Error>
    where
        F: Fn(&'v Value) -> Option<T>;
}

impl TypedArrayPointer for Option<&Value> {
    fn as_typed_array<'v, F, T>(&'v self, f: F) -> Result<Vec<T>, Error>
    where
        F: Fn(&'v Value) -> Option<T>,
    {
        let r: Option<Result<Vec<T>, Error>> = self.map(Value::as_array).flatten().map(|v| {
            v.iter()
                .map(|i| f(i).ok_or(Error::InvalidValue(format!("{:?}", i))))
                .collect::<Result<Vec<T>, Error>>()
        });

        r.ok_or(Error::InvalidValue(
            "Unable to convert to an array".to_string(),
        ))?
    }
}

pub fn try_from_path(path: &PathBuf) -> Result<Value, Error> {
    let contents = std::fs::read_to_string(&path).map_err(Error::from)?;
    let ext = match path.extension().map(|e| e.to_str()).flatten() {
        Some(ext) => ext,
        None => DEFAULT_FORMAT,
    };

    eprintln!("Parsing agreement at {}", path.display());
    match ext.to_lowercase().as_str() {
        "json" => try_from_json(&contents),
        "yaml" => try_from_yaml(&contents),
        _ => Err(Error::UnsupportedFormat(ext.to_string())),
    }
}

pub fn try_from_json<S: AsRef<str>>(contents: S) -> Result<Value, Error> {
    Ok(expand(
        serde_json::from_str::<Value>(contents.as_ref()).map_err(Error::from)?,
    ))
}

pub fn try_from_yaml<S: AsRef<str>>(contents: S) -> Result<Value, Error> {
    Ok(expand(
        serde_yaml::from_str::<Value>(contents.as_ref()).map_err(Error::from)?,
    ))
}

pub fn expand(value: Value) -> Value {
    match value {
        Value::Object(m) => {
            let mut new_map = Map::new();

            for (k, v) in m.into_iter() {
                let mut parts: Vec<&str> = k.split('.').collect();

                let ki = parts.remove(0).to_string();
                let vi = expand_obj(parts, expand(v));

                if let Some(ve) = new_map.get_mut(&ki) {
                    merge_obj(ve, vi);
                } else {
                    new_map.insert(ki, vi);
                }
            }

            Value::Object(new_map)
        }
        Value::Array(array) => Value::Array(array.into_iter().map(expand).collect()),
        value => value,
    }
}

fn expand_obj(mut parts: Vec<&str>, v: Value) -> Value {
    match parts.len() {
        0 => v,
        _ => {
            let f = parts.remove(0).to_string();
            Value::Object(vec![(f, expand_obj(parts, v))].into_iter().collect())
        }
    }
}

fn merge_obj(a: &mut Value, b: Value) {
    match (a, b) {
        (a @ &mut Value::Object(_), Value::Object(b)) => {
            let a = a.as_object_mut().unwrap();
            for (k, v) in b {
                merge_obj(a.entry(k).or_insert(Value::Null), v);
            }
        }
        (a, Value::Object(mut b)) => {
            match a {
                Value::Null => (),
                _ => {
                    b.insert(PROPERTY_TAG.to_string(), a.clone());
                }
            }
            *a = Value::Object(b);
        }
        (a @ &mut Value::Object(_), b) => match b {
            Value::Null => (),
            _ => {
                let a = a.as_object_mut().unwrap();
                a.insert(PROPERTY_TAG.to_string(), b.clone());
            }
        },
        (a, b) => *a = b,
    }
}

fn properties(prefix: String, result: &mut Map<String, Value>, value: Value) {
    match value {
        Value::Object(m) => {
            for (k, v) in m.into_iter() {
                if k.as_str() == PROPERTY_TAG {
                    result.insert(prefix.clone(), v);
                    continue;
                }
                let pi = match prefix.is_empty() {
                    true => k,
                    _ => format!("{}.{}", prefix, k),
                };
                properties(pi, result, v);
            }
        }
        v => {
            result.insert(prefix, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TypedPointer;
    use super::*;

    const YAML: &str = r#"
properties:
  golem:
    inf:
      mem.gib: 0.5
      storage.gib: 5.0
    node:
      id.name: dany
    activity.caps:
        transfer.protocol:
          - http
          - https
          - container
    com:
      scheme: payu
      scheme.payu:
        interval_sec: 60
      pricing:
        model: linear
        model.linear:
          coeffs: [0, 0.01, 0.0016]
      usage:
        vector: ["golem.usage.duration_sec", "golem.usage.cpu_sec"]
constraints: |
  ()
"#;

    const JSON: &str = r#"
{
	"properties": {
		"golem": {
			"inf": {
				"mem.gib": 0.5,
				"storage.gib": 5
			},
			"node": {
				"id.name": "dany"
			},
			"activity.caps": {
				"transfer.protocol": [
					"http",
					"https",
					"container"
				]
			},
			"com": {
				"scheme": "payu",
				"scheme.payu": {
					"interval_sec": 60
				},
				"pricing": {
					"model": "linear",
					"model.linear": {
						"coeffs": [
							0,
							0.01,
							0.0016
						]
					}
				},
				"usage": {
					"vector": [
						"golem.usage.duration_sec",
						"golem.usage.cpu_sec"
					]
				}
			}
		}
	},
	"constraints": "()\n"
}
"#;

    fn check_values(o: &serde_json::Value) {
        assert_eq!(
            o.pointer("/properties/golem/inf/mem/gib")
                .as_typed(Value::as_f64)
                .unwrap(),
            0.5f64
        );
        assert_eq!(
            o.pointer("/properties/golem/inf/storage/gib")
                .as_typed(Value::as_f64)
                .unwrap(),
            5f64
        );
        assert_eq!(
            o.pointer("/properties/golem/node/id/name")
                .as_typed(Value::as_str)
                .unwrap(),
            "dany"
        );
        assert_eq!(
            o.pointer("/properties/golem/activity/caps/transfer/protocol")
                .as_typed_array(Value::as_str)
                .unwrap(),
            vec!["http", "https", "container",]
        );
        assert_eq!(
            o.pointer(&format!("/properties/golem/com/scheme/{}", PROPERTY_TAG))
                .as_typed(Value::as_str)
                .unwrap(),
            "payu"
        );
        assert_eq!(
            o.pointer("/properties/golem/com/scheme/payu/interval_sec")
                .as_typed(Value::as_i64)
                .unwrap(),
            60i64
        );
        assert_eq!(
            o.pointer("/properties/golem/com/scheme/payu/interval_sec")
                .as_typed(Value::as_f64)
                .unwrap(),
            60f64
        );
        assert_eq!(
            o.pointer("/properties/golem/com/pricing/model/linear/coeffs")
                .as_typed_array(Value::as_f64)
                .unwrap(),
            vec![0f64, 0.01f64, 0.0016f64]
        );
        assert_eq!(
            o.pointer("/properties/golem/com/usage/vector")
                .as_typed_array(Value::as_str)
                .unwrap(),
            vec!["golem.usage.duration_sec", "golem.usage.cpu_sec",]
        );
    }

    #[test]
    fn json() {
        let offer = try_from_json(JSON).unwrap();
        check_values(&offer);
    }

    #[test]
    fn yaml() {
        let offer = try_from_yaml(YAML).unwrap();
        check_values(&offer);
    }

    #[test]
    fn expand_and_merge() {
        let mut f = try_from_json(
            r#"
{
   "first": {
      "inner": 1,
      "expand.me.please": 0,
      "other": {
         "nested":2
      }
   }
}"#,
        )
        .unwrap();

        let s = try_from_json(
            r#"
{
   "first":{
      "inner": {
         "i": 2
      },
      "expand.also": 1,
      "other": 3
   }
}"#,
        )
        .unwrap();

        let e = try_from_json(
            r#"
{
   "first": {
      "inner": {
         "i": 2,
         "@tag": 1
      },
      "expand": {
         "me": {
            "please": 0
         },
         "also": 1
      },
      "other": {
        "@tag": 3,
        "nested": 2
      }
   }
}"#,
        )
        .unwrap();

        merge_obj(&mut f, s);
        assert_eq!(f, e);
    }

    #[test]
    fn expand_json_error() {
        let actual = try_from_json(
            r#"
            {
                "a": { "b.c": 1 },
                "a.b": 2
            }"#,
        )
        .unwrap();
        let expected = serde_json::json!({
            "a": { "b": { "c": 1, PROPERTY_TAG: 2 } },
        });
        assert_eq!(actual, expected);
    }

    #[test]
    fn expand_json_good() {
        let actual = try_from_json(
            r#"
            {
                "a.b": { "c": 1 },
                "a": { "b": 2 }
            }"#,
        )
        .unwrap();
        let expected = serde_json::json!({
            "a": { "b": { "c": 1, PROPERTY_TAG: 2 } },
        });
        assert_eq!(actual, expected);
    }

    #[test]
    fn properties_json() {
        let source = serde_json::json!({
            "a": {
                "b": {
                    "c": 1,
                    PROPERTY_TAG: 2
                }
            },
            "r": [123, "string"]
        });

        let mut map = Map::new();
        properties(String::new(), &mut map, source);
        assert_eq!(map.get("a.b.c").unwrap(), 1);
        assert_eq!(map.get("a.b").unwrap(), 2);
        assert_eq!(map.get("r").unwrap(), &serde_json::json!([123, "string"]));
    }
}
