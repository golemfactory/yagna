use crate::Result;
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use ya_model::activity::{ExeScriptCommand, State};

pub trait InnerEq<T: Eq> {
    fn inner_eq(&self, v: &T) -> bool;
}

impl<T: Eq> InnerEq<T> for Option<T> {
    fn inner_eq(&self, v: &T) -> bool {
        self.as_ref().map(|i| i == v).unwrap_or(false)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<(State, String)>")]
pub struct Command(pub ExeScriptCommand);

#[derive(Clone, Debug, Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateState {
    pub state: State,
}
