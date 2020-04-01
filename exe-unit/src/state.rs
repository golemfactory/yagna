use crate::notify::Notify;
use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;
use ya_core_model::activity::Exec;
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
    pub batches: HashMap<String, Exec>,
    batch_results: HashMap<String, Vec<ExeScriptCommandResult>>,
    batch_notifiers: HashMap<String, Notify<usize>>,
}

impl ExeUnitState {
    pub fn batch_results(&self, batch_id: &String) -> Vec<ExeScriptCommandResult> {
        match self.batch_results.get(batch_id) {
            Some(vec) => vec.clone(),
            None => Vec::new(),
        }
    }

    pub fn push_batch_result(&mut self, batch_id: String, result: ExeScriptCommandResult) {
        let idx = result.index as usize;
        match self.batch_results.get_mut(&batch_id) {
            Some(vec) => vec.push(result),
            None => {
                self.batch_results.insert(batch_id.clone(), vec![result]);
            }
        }
        self.notifier(&batch_id).notify(idx);
    }

    pub fn notifier(&mut self, batch_id: &String) -> &mut Notify<usize> {
        let notifiers = &mut self.batch_notifiers;
        if !notifiers.contains_key(batch_id) {
            notifiers.insert(batch_id.clone(), Notify::default());
        }
        notifiers.get_mut(batch_id).unwrap()
    }
}

impl Default for ExeUnitState {
    fn default() -> Self {
        ExeUnitState {
            inner: StatePair::default(),
            running_command: None,
            batches: HashMap::new(),
            batch_results: HashMap::new(),
            batch_notifiers: HashMap::new(),
        }
    }
}
