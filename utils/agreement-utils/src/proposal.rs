use ya_client_model::market::proposal::State;
use ya_client_model::market::{NewProposal, Proposal};
use ya_client_model::NodeId;

use crate::agreement::{expand, flatten, try_from_path, TypedPointer};
use crate::{Error, OfferTemplate};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposalView {
    pub content: OfferTemplate,
    pub id: String,
    pub issuer: NodeId,
    pub state: State,
    pub timestamp: DateTime<Utc>,
}

impl ProposalView {
    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        self.content.pointer(pointer)
    }

    pub fn pointer_mut(&mut self, pointer: &str) -> Option<&mut Value> {
        self.content.properties.pointer_mut(pointer)
    }

    pub fn pointer_typed<'a, T: Deserialize<'a>>(&self, pointer: &str) -> Result<T, Error> {
        self.content.pointer_typed(pointer)
    }

    pub fn properties<'a, T: Deserialize<'a>>(
        &self,
        pointer: &str,
    ) -> Result<HashMap<String, T>, Error> {
        self.content.properties_at(pointer)
    }

    pub fn get_property<'a, T: Deserialize<'a>>(&self, property: &str) -> Result<T, Error> {
        let pointer = format!("/{}", property.replace(".", "/"));
        self.pointer_typed(pointer.as_str())
    }

    pub fn remove_property(&mut self, pointer: &str) -> Result<(), Error> {
        let path: Vec<&str> = pointer.split('/').collect();
        Ok(
            // Path should start with '/', so we must omit first element, which will be empty.
            remove_property_impl(&mut self.content.properties, &path[1..]).map_err(
                |e| match e {
                    Error::NoKey(_) => Error::NoKey(pointer.to_string()),
                    _ => e,
                },
            )?,
        )
    }
}

pub(crate) fn remove_property_impl(
    value: &mut serde_json::Value,
    path: &[&str],
) -> Result<(), Error> {
    assert_ne!(path.len(), 0);
    if path.len() == 1 {
        remove_value(value, path[0])?;
        Ok(())
    } else {
        let nested_value = value
            .pointer_mut(&["/", path[0]].concat())
            .ok_or(Error::NoKey(path[0].to_string()))?;
        remove_property_impl(nested_value, &path[1..])?;

        // Check if nested_value contains anything else.
        // We remove this key if Value was empty.
        match nested_value {
            Value::Array(array) => {
                if array.is_empty() {
                    remove_value(value, path[0]).ok();
                }
            }
            Value::Object(object) => {
                if object.is_empty() {
                    remove_value(value, path[0]).ok();
                }
            }
            _ => (),
        };
        Ok(())
    }
}

pub(crate) fn remove_value(value: &mut Value, name: &str) -> Result<Value, Error> {
    Ok(match value {
        Value::Array(array) => array.remove(
            name.parse::<usize>()
                .map_err(|_| Error::InvalidValue(name.to_string()))?,
        ),
        Value::Object(object) => object
            .remove(name)
            .ok_or(Error::InvalidValue(name.to_string()))?,
        _ => Err(Error::InvalidValue(name.to_string()))?,
    })
}

impl TryFrom<Value> for ProposalView {
    type Error = Error;

    fn try_from(mut value: Value) -> Result<Self, Self::Error> {
        let offer = OfferTemplate {
            properties: expand(
                value
                    .pointer_mut("/properties")
                    .map(Value::take)
                    .unwrap_or(Value::Null),
            ),
            constraints: value
                .pointer("/constraints")
                .as_typed(Value::as_str)?
                .to_owned(),
        };
        Ok(ProposalView {
            content: offer,
            id: value
                .pointer("/proposalId")
                .as_typed(Value::as_str)?
                .to_owned(),
            issuer: value
                .pointer("/issuerId")
                .as_typed(Value::as_str)?
                .parse()
                .map_err(|e| Error::InvalidValue(format!("Can't parse NodeId. {}", e)))?,
            state: serde_json::from_value(
                value
                    .pointer("/state")
                    .cloned()
                    .ok_or(Error::NoKey(format!("state")))?,
            )
            .map_err(|e| Error::InvalidValue(format!("Can't deserialize State. {}", e)))?,
            timestamp: value
                .pointer("/timestamp")
                .as_typed(Value::as_str)?
                .parse()
                .map_err(|e| Error::InvalidValue(format!("Can't parse timestamp. {}", e)))?,
        })
    }
}

impl From<ProposalView> for NewProposal {
    fn from(proposal: ProposalView) -> Self {
        NewProposal {
            properties: serde_json::Value::Object(flatten(proposal.content.properties)),
            constraints: proposal.content.constraints,
        }
    }
}

impl TryFrom<&PathBuf> for ProposalView {
    type Error = Error;

    fn try_from(path: &PathBuf) -> Result<Self, Self::Error> {
        Self::try_from(try_from_path(path)?)
    }
}

impl TryFrom<&Proposal> for ProposalView {
    type Error = Error;

    fn try_from(proposal: &Proposal) -> Result<Self, Self::Error> {
        Self::try_from(expand(serde_json::to_value(proposal)?))
    }
}
