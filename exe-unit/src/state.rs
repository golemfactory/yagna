use crate::notify::Notify;
use futures::channel::oneshot;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
pub use ya_client_model::activity::activity_state::{State, StatePair};
use ya_client_model::activity::{ExeScriptCommandResult, ExeScriptCommandState};
use ya_core_model::activity::Exec;

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
    pub batch_control: HashMap<String, Option<oneshot::Sender<()>>>,
    batch_results: HashMap<String, Vec<ExeScriptCommandResult>>,
    batch_notifiers: HashMap<String, Notify<usize>>,
}

impl ExeUnitState {
    pub fn report(&self) -> ExeUnitReport {
        let mut report = ExeUnitReport::new();

        self.batches.iter().for_each(|(batch_id, exec)| {
            let total = exec.exe_script.len();
            match self.batch_results.get(batch_id) {
                Some(results) => {
                    let done = results.len();
                    if done == total {
                        report.batches_done += 1;
                    } else {
                        report.batches_pending += 1;
                    }
                    report.cmds_done += done;
                    report.cmds_pending += total - done;
                }
                None => {
                    report.batches_pending += 1;
                    report.cmds_pending += total;
                }
            }
        });

        report
    }

    pub fn batch_results(&self, batch_id: &str) -> Vec<ExeScriptCommandResult> {
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

    pub fn notifier(&mut self, batch_id: &str) -> &mut Notify<usize> {
        let notifiers = &mut self.batch_notifiers;
        if !notifiers.contains_key(batch_id) {
            notifiers.insert(batch_id.to_owned(), Notify::default());
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
            batch_control: HashMap::new(),
            batch_results: HashMap::new(),
            batch_notifiers: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExeUnitReport {
    batches_done: usize,
    batches_pending: usize,
    cmds_done: usize,
    cmds_pending: usize,
}

impl ExeUnitReport {
    pub fn new() -> Self {
        ExeUnitReport {
            batches_done: 0,
            batches_pending: 0,
            cmds_done: 0,
            cmds_pending: 0,
        }
    }
}

impl std::fmt::Display for ExeUnitReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&serde_json::to_string(self).unwrap())
    }
}
