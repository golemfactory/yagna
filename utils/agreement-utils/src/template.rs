use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::agreement::flatten;
use crate::Error;

/// TODO: Could we use Constraints instead of String?? This would require parsing string.
///  It is complicated, but we could use code from resolver to do this.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OfferTemplate {
    pub properties: Value,
    pub constraints: String,
}

impl Default for OfferTemplate {
    fn default() -> Self {
        OfferTemplate {
            properties: Value::Object(Map::new()),
            constraints: String::new(),
        }
    }
}

impl OfferTemplate {
    pub fn new(properties: Value) -> Self {
        OfferTemplate {
            properties: Value::Object(flatten(properties)),
            constraints: String::new(),
        }
    }

    pub fn patch(mut self, template: Self) -> Self {
        patch(&mut self.properties, template.properties);
        self.add_constraints(template.constraints);
        self
    }

    pub fn property(&self, property: &str) -> Option<&Value> {
        self.properties.as_object().unwrap().get(property)
    }

    pub fn set_property(&mut self, key: impl ToString, value: Value) {
        let properties = self.properties.as_object_mut().unwrap();
        properties.insert(key.to_string(), value);
    }

    pub fn add_constraints(&mut self, constraints: String) {
        if self.constraints.is_empty() {
            self.constraints = constraints;
        } else {
            self.constraints = format!("(& {} {})", self.constraints, constraints);
        }
    }

    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        self.properties.pointer(pointer)
    }

    pub fn pointer_typed<'a, T: Deserialize<'a>>(&self, pointer: &str) -> Result<T, Error> {
        let value = self
            .properties
            .pointer(pointer)
            .ok_or(Error::NoKey(pointer.to_string()))?
            .clone();
        Ok(<T as Deserialize>::deserialize(value)
            .map_err(|error| Error::UnexpectedType(pointer.to_string(), error))?)
    }

    pub fn properties_at<'a, T: Deserialize<'a>>(
        &self,
        pointer: &str,
    ) -> Result<HashMap<String, T>, Error> {
        let value = self
            .pointer(pointer)
            .ok_or(Error::NoKey(pointer.to_string()))?;

        let map = flatten(value.clone())
            .into_iter()
            .filter_map(|(k, v)| match <T as Deserialize>::deserialize(v) {
                Ok(v) => Some((k, v)),
                Err(_) => None,
            })
            .collect();
        Ok(map)
    }
}

pub fn patch(a: &mut Value, b: Value) {
    match (a, b) {
        (a @ &mut Value::Object(_), Value::Object(b)) => {
            let a = a.as_object_mut().unwrap();
            for (k, v) in b {
                patch(a.entry(k).or_insert(Value::Null), v);
            }
        }
        (a, b) => *a = b,
    }
}
