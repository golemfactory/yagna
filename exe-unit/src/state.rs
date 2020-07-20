use crate::error::Error;
use crate::notify::Notify;
use actix::Arbiter;
use futures::channel::oneshot;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::broadcast;
pub use ya_client_model::activity::activity_state::{State, StatePair};
use ya_client_model::activity::{CommandResult, ExeScriptCommandResult, ExeScriptCommandState};
use ya_core_model::activity::{Exec, RuntimeEvent};

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
    pub last_batch: Option<String>,
    pub batches: HashMap<String, Batch>,
}

impl ExeUnitState {
    pub fn start_batch(&mut self, script: Exec, control: oneshot::Sender<()>) {
        let batch_id = script.batch_id.clone();
        self.batches.insert(batch_id, Batch::new(script, control));
    }

    pub fn report(&self) -> ExeUnitReport {
        let mut report = ExeUnitReport::new();
        self.batches.values().for_each(|batch| {
            let total = batch.total();
            let done = batch.done();
            match done == total {
                true => report.batches_done += 1,
                false => report.batches_pending += 1,
            }
            report.cmds_done += done;
            report.cmds_pending += total - done;
        });
        report
    }
}

impl Default for ExeUnitState {
    fn default() -> Self {
        ExeUnitState {
            inner: Default::default(),
            batches: Default::default(),
            last_batch: None,
        }
    }
}

pub struct Batch {
    pub script: Exec,
    pub control: Option<oneshot::Sender<()>>,
    pub results: Vec<CommandState>,
    pub notifier: Notify<usize>,
    pub stream: Broadcast<RuntimeEvent>,
}

impl Batch {
    pub fn new(script: Exec, control: oneshot::Sender<()>) -> Self {
        Batch {
            script,
            control: Some(control),
            results: Default::default(),
            notifier: Default::default(),
            stream: Default::default(),
        }
    }

    pub fn total(&self) -> usize {
        self.script.exe_script.len()
    }

    pub fn done(&self) -> usize {
        self.results
            .iter()
            .take_while(|r| r.result.is_some())
            .count()
    }
}

impl Batch {
    pub fn started(&mut self, idx: usize) -> Result<(), Error> {
        self.result(idx)?;
        Ok(())
    }

    pub fn finished(
        &mut self,
        idx: usize,
        code: i32,
        message: Option<String>,
    ) -> Result<(), Error> {
        let cmd_result = self.result(idx)?;
        cmd_result.result = Some(match code {
            0 => CommandResult::Ok,
            _ => CommandResult::Error,
        });
        if message.is_some() {
            cmd_result.stderr = message;
        }
        self.notifier.notify(idx as usize);
        Ok(())
    }

    pub fn push_stdout(&mut self, idx: usize, out: String) -> Result<(), Error> {
        let cmd_result = self.result(idx)?;
        match &mut cmd_result.stdout {
            Some(stdout) => stdout.push_str(&out),
            _ => cmd_result.stdout = Some(out),
        }
        Ok(())
    }

    pub fn push_stderr(&mut self, idx: usize, out: String) -> Result<(), Error> {
        let cmd_result = self.result(idx)?;
        match &mut cmd_result.stderr {
            Some(stderr) => stderr.push_str(&out),
            _ => cmd_result.stderr = Some(out),
        }
        Ok(())
    }

    pub fn running_cmd(&self) -> Option<ExeScriptCommandState> {
        let result = self
            .results
            .iter()
            .enumerate()
            .filter(|(_, r)| r.result.is_none())
            .next()
            .map(|(i, r)| (i, r.message()));
        match result {
            Some((idx, msg)) => self.script.exe_script.get(idx).map(|c| {
                let mut state = ExeScriptCommandState::from(c.clone());
                state.progress = msg;
                state
            }),
            _ => None,
        }
    }

    pub fn results(&self) -> Vec<ExeScriptCommandResult> {
        let batch_size = self.script.exe_script.len();
        self.results
            .iter()
            .enumerate()
            .take_while(|(_, r)| r.result.is_some())
            .map(|(i, r)| {
                let result = r.result.clone().unwrap();
                ExeScriptCommandResult {
                    index: i as u32,
                    result,
                    is_batch_finished: i == batch_size - 1 || result == CommandResult::Error,
                    message: r.message(),
                }
            })
            .collect::<Vec<_>>()
    }

    #[inline]
    fn result(&mut self, idx: usize) -> Result<&mut CommandState, Error> {
        while self.results.len() < idx + 1 {
            self.results.push(Default::default());
        }
        Ok(self.results.get_mut(idx).unwrap())
    }
}

pub struct Broadcast<T: Clone> {
    sender: Option<broadcast::Sender<T>>,
}

impl<T: Clone + 'static> Broadcast<T> {
    pub fn initialized(&self) -> bool {
        self.sender.is_some()
    }

    pub fn sender(&mut self) -> &mut broadcast::Sender<T> {
        if !self.initialized() {
            self.initialize();
        }
        self.sender.as_mut().unwrap()
    }

    pub fn receiver(&mut self) -> broadcast::Receiver<T> {
        self.sender().subscribe()
    }

    fn initialize(&mut self) {
        let (tx, rx) = broadcast::channel(4);
        Arbiter::spawn(rx.for_each(|_| async { () }));
        self.sender = Some(tx);
    }
}

impl<T: Clone> Default for Broadcast<T> {
    fn default() -> Self {
        Broadcast { sender: None }
    }
}

pub struct CommandState {
    pub result: Option<CommandResult>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

impl CommandState {
    pub fn message(&self) -> Option<String> {
        match (&self.stdout, &self.stderr) {
            (None, None) => None,
            (Some(stdout), None) => Some(format!("stdout: {}", stdout)),
            (None, Some(stderr)) => Some(format!("stderr: {}", stderr)),
            (Some(stdout), Some(stderr)) => Some(format!("stdout: {}\nstderr: {}", stdout, stderr)),
        }
    }
}

impl Default for CommandState {
    fn default() -> Self {
        CommandState {
            result: None,
            stdout: None,
            stderr: None,
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
