use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;
pub use ya_model::activity::activity_state::{State, StatePair};
use ya_model::activity::{ExeScriptCommandResult, ExeScriptCommandState};

#[derive(Error, Debug, Serialize)]
pub enum StateError {
    #[error("Busy: {0:?}")]
    Busy(StatePair),
    #[error("Invalid state: {0:?}")]
    InvalidState(StatePair),
    #[error("Unexpected state: {current:?}, expected {expected:?}")]
    UnexpectedState {
        current: StatePair,
        expected: StatePair,
    },
}

pub struct ExeUnitState {
    pub inner: StatePair,
    pub running_command: Option<ExeScriptCommandState>,
    batch_results: HashMap<String, Vec<ExeScriptCommandResult>>,
}

impl ExeUnitState {
    pub fn batch_results(&self, batch_id: &String) -> Vec<ExeScriptCommandResult> {
        match self.batch_results.get(batch_id) {
            Some(vec) => vec.clone(),
            None => Vec::new(),
        }
    }

    pub fn push_batch_result(&mut self, batch_id: String, result: ExeScriptCommandResult) {
        match self.batch_results.get_mut(&batch_id) {
            Some(vec) => vec.push(result),
            None => {
                self.batch_results.insert(batch_id, vec![result]);
            }
        }
    }
}

impl Default for ExeUnitState {
    fn default() -> Self {
        ExeUnitState {
            inner: StatePair::default(),
            batch_results: HashMap::new(),
            running_command: None,
        }
    }
}
