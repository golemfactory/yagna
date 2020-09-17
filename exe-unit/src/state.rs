use crate::error::Error;
use crate::notify::Notify;
use crate::output::CapturedOutput;
use actix::Arbiter;
use futures::channel::oneshot;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::broadcast;
pub use ya_client_model::activity::activity_state::{State, StatePair};
use ya_client_model::activity::{
    Capture, CommandOutput, CommandResult, ExeScriptCommand, ExeScriptCommandResult,
    ExeScriptCommandState, RuntimeEvent, RuntimeEventKind,
};
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

pub(crate) struct ExeUnitState {
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

pub(crate) struct Batch {
    pub exec: Exec,
    pub results: Vec<CommandState>,
    pub control: Option<oneshot::Sender<()>>,
    pub notifier: Notify<usize>,
    pub stream: Broadcast<RuntimeEvent>,
}

impl Batch {
    pub fn new(exec: Exec, control: oneshot::Sender<()>) -> Self {
        Batch {
            exec,
            results: Default::default(),
            control: Some(control),
            notifier: Default::default(),
            stream: Default::default(),
        }
    }

    pub fn total(&self) -> usize {
        self.exec.exe_script.len()
    }

    pub fn done(&self) -> usize {
        self.results
            .iter()
            .take_while(|r| r.result.is_some())
            .count()
    }
}

impl Batch {
    pub fn handle_event(&mut self, event: RuntimeEvent) -> Result<(), Error> {
        let idx = event.index;
        let stream_event = match event.kind.clone() {
            RuntimeEventKind::Started { command: _ } => {
                self.state(idx).map(|_| ())?;
                Some(event)
            }
            RuntimeEventKind::Finished {
                return_code,
                message,
            } => {
                let state = self.state(idx)?;
                state.message = message.clone();
                state.result = Some(match return_code {
                    0 => CommandResult::Ok,
                    _ => CommandResult::Error,
                });
                self.notifier.notify(idx as usize);
                Some(event)
            }
            RuntimeEventKind::StdOut(out) => {
                let state = self.state(idx)?;
                let output = state.stdout.write(output_bytes(&out));
                output
                    .filter(|_| state.stdout.stream)
                    .map(|o| RuntimeEvent {
                        kind: RuntimeEventKind::StdOut(o),
                        ..event
                    })
            }
            RuntimeEventKind::StdErr(out) => {
                let state = self.state(idx)?;
                let output = state.stderr.write(output_bytes(&out));
                output
                    .filter(|_| state.stderr.stream)
                    .map(|o| RuntimeEvent {
                        kind: RuntimeEventKind::StdErr(o),
                        ..event
                    })
            }
        };

        if let Some(evt) = stream_event {
            if self.stream.initialized() {
                self.stream
                    .sender()
                    .send(evt)
                    .map_err(|e| Error::runtime(format!("output stream error: {:?}", e)))?;
            }
        }
        Ok(())
    }
}

impl Batch {
    pub fn running_command(&self) -> Option<ExeScriptCommandState> {
        let result = self
            .results
            .iter()
            .enumerate()
            .filter(|(_, s)| s.result.is_none())
            .next()
            .map(|(i, s)| (i, s.message.clone()));

        match result {
            Some((idx, msg)) => self.exec.exe_script.get(idx).map(|c| {
                let mut state = ExeScriptCommandState::from(c.clone());
                state.progress = msg;
                state
            }),
            _ => None,
        }
    }

    pub fn results(&self) -> Vec<ExeScriptCommandResult> {
        let last_idx = self.exec.exe_script.len() - 1;
        self.results
            .iter()
            .enumerate()
            .take_while(|(_, s)| s.result.is_some())
            .map(|(idx, s)| {
                let result = s.result.clone().unwrap();
                let is_batch_finished = idx == last_idx || result == CommandResult::Error;
                ExeScriptCommandResult {
                    index: idx as u32,
                    result,
                    stdout: s.stdout.output(),
                    stderr: s.stderr.output(),
                    message: s.message.clone(),
                    is_batch_finished,
                }
            })
            .collect::<Vec<_>>()
    }

    #[inline]
    fn state(&mut self, idx: usize) -> Result<&mut CommandState, Error> {
        let exe_script = &self.exec.exe_script;
        let available = self.results.len();

        if idx >= exe_script.len() {
            return Err(Error::runtime(format!("unknown command index: {}", idx)));
        } else if idx >= available {
            let iter = exe_script
                .iter()
                .skip(available)
                .take(idx - available + 1)
                .map(|cmd| match cmd {
                    ExeScriptCommand::Run { capture, .. } => CommandState::from(capture),
                    _ => CommandState::all(),
                });
            self.results.extend(iter);
        }
        Ok(self.results.get_mut(idx).unwrap())
    }
}

pub(crate) struct Broadcast<T: Clone> {
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

pub(crate) struct CommandState {
    pub result: Option<CommandResult>,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub message: Option<String>,
}

impl CommandState {
    pub fn all() -> Self {
        CommandState {
            result: None,
            stdout: CapturedOutput::all(),
            stderr: CapturedOutput::all(),
            message: None,
        }
    }

    pub fn discard() -> Self {
        CommandState {
            result: None,
            stdout: CapturedOutput::discard(),
            stderr: CapturedOutput::discard(),
            message: None,
        }
    }
}

impl<'c> From<&'c Option<Capture>> for CommandState {
    fn from(capture: &'c Option<Capture>) -> Self {
        capture
            .as_ref()
            .map(CommandState::from)
            .unwrap_or_else(CommandState::discard)
    }
}

impl<'c> From<&'c Capture> for CommandState {
    fn from(capture: &'c Capture) -> Self {
        CommandState {
            result: None,
            stdout: CapturedOutput::from(capture.stdout.clone()),
            stderr: CapturedOutput::from(capture.stderr.clone()),
            message: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ExeUnitReport {
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

fn output_bytes(output: &CommandOutput) -> &[u8] {
    match output {
        CommandOutput::Bin(vec) => vec.as_slice(),
        CommandOutput::Str(string) => string.as_bytes(),
    }
}
