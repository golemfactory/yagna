use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, StreamExt, TryStreamExt};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;

pub use ya_client_model::activity::activity_state::{State, StatePair};
use ya_client_model::activity::exe_script_command::Network;
use ya_client_model::activity::*;
use ya_core_model::activity::Exec;
use ya_utils_networking::vpn::common::{to_ip, to_net};
use ya_utils_networking::vpn::Error as NetError;

use crate::error::Error;
use crate::manifest::ManifestContext;
use crate::notify::Notify;
use crate::output::CapturedOutput;
use crate::runtime::RuntimeMode;

fn invalid_state_err_msg(state_pair: &StatePair) -> String {
    match state_pair {
        StatePair(State::Initialized, None) => {
            format!("Activity is initialized - deploy() command is expected now")
        }
        StatePair(State::Deployed, None) => {
            format!("Activity is deployed - start() command is expected now")
        }
        StatePair(State::Ready, None) => format!("Cannot send command after a successful start()"),
        _ => format!(
            "This command is not allowed when activity is in the {:?} state",
            state_pair.0
        ),
    }
}

#[derive(Error, Debug, Serialize)]
pub enum StateError {
    #[error("Busy: {0:?}")]
    Busy(StatePair),
    #[error("{}", invalid_state_err_msg(.0))]
    InvalidState(StatePair),
    #[error("Unexpected state: {current:?}, expected {expected:?}")]
    UnexpectedState {
        current: StatePair,
        expected: StatePair,
    },
}

#[derive(Debug, Default)]
pub struct Supervision {
    pub hardware: bool,
    pub image: bool,
    pub manifest: ManifestContext,
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
        let stream_event = match &event.kind {
            RuntimeEventKind::Started { command: _ } => {
                self.state(idx).map(|_| ())?;
                Some(event)
            }
            RuntimeEventKind::Finished {
                return_code,
                message,
            } => {
                let state = self.state(idx)?;
                state.date = Utc::now();
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
                let output = state.stdout.write(output_bytes(out));
                output
                    .filter(|_| state.stdout.stream)
                    .map(|o| RuntimeEvent {
                        kind: RuntimeEventKind::StdOut(o),
                        ..event
                    })
            }
            RuntimeEventKind::StdErr(out) => {
                let state = self.state(idx)?;
                let output = state.stderr.write(output_bytes(out));
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

    pub fn results(&self, cmd_idx: Option<usize>) -> Vec<ExeScriptCommandResult> {
        let last_idx = cmd_idx.unwrap_or(self.exec.exe_script.len() - 1);
        self.results
            .iter()
            .enumerate()
            .take_while(|(idx, s)| *idx <= last_idx && s.result.is_some())
            .map(|(idx, s)| {
                let result = s.result.clone().unwrap();
                let output = cmd_idx.as_ref().map(|i| *i == idx).unwrap_or(true);
                ExeScriptCommandResult {
                    index: idx as u32,
                    result,
                    stdout: if output { s.stdout.output() } else { None },
                    stderr: if output { s.stderr.output() } else { None },
                    message: s.message.clone(),
                    is_batch_finished: idx == last_idx || result == CommandResult::Error,
                    event_date: s.date,
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

impl<T: Clone + Send + 'static> Broadcast<T> {
    pub fn initialized(&self) -> bool {
        self.sender.is_some()
    }

    pub fn sender(&mut self) -> &mut broadcast::Sender<T> {
        if !self.initialized() {
            self.initialize();
        }
        self.sender.as_mut().unwrap()
    }

    pub fn receiver(&mut self) -> mpsc::UnboundedReceiver<Result<T, Error>> {
        let (tx, rx) = mpsc::unbounded::<Result<T, _>>();
        let mut txc = tx.clone();
        let receiver = self.sender().subscribe();
        tokio::task::spawn_local(async move {
            if let Err(e) = tokio_stream::wrappers::BroadcastStream::new(receiver)
                .map_err(Error::runtime)
                .forward(
                    tx.sink_map_err(Error::runtime)
                        .with(|v| futures::future::ready(Ok(Ok(v)))),
                )
                .await
            {
                let msg = format!("stream error: {}", e);
                log::error!("Broadcast output error: {}", msg);
                let _ = txc.send(Err::<T, _>(Error::runtime(msg))).await;
            }
        });
        rx
    }

    fn initialize(&mut self) {
        let (tx, rx) = broadcast::channel(16);
        let receiver = tokio_stream::wrappers::BroadcastStream::new(rx);
        tokio::task::spawn_local(receiver.for_each(|_| async {  }));
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
    pub date: DateTime<Utc>,
}

impl CommandState {
    fn new(stdout: CapturedOutput, stderr: CapturedOutput) -> Self {
        CommandState {
            result: None,
            stdout,
            stderr,
            message: None,
            date: Utc::now(),
        }
    }

    pub fn all() -> Self {
        Self::new(CapturedOutput::all(), CapturedOutput::all())
    }

    pub fn discard() -> Self {
        Self::new(CapturedOutput::discard(), CapturedOutput::discard())
    }

    #[allow(dead_code)]
    pub fn repr(&self) -> CommandStateRepr {
        CommandStateRepr {
            result: self.result.clone(),
            stdout: self.stdout.output_string(),
            stderr: self.stderr.output_string(),
            message: self.message.clone(),
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
        Self::new(capture.stdout.clone().into(), capture.stderr.clone().into())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandStateRepr {
    pub result: Option<CommandResult>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub(crate) struct ExeUnitReport {
    batches_done: usize,
    batches_pending: usize,
    cmds_done: usize,
    cmds_pending: usize,
}

impl ExeUnitReport {
    pub fn new() -> Self {
        Default::default()
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

#[derive(Clone, Default)]
pub(crate) struct Deployment {
    pub runtime_mode: RuntimeMode,
    pub task_package: Option<PathBuf>,
    pub networks: HashMap<String, DeploymentNetwork>,
    pub hosts: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DeploymentNetwork {
    pub network: IpNet,
    pub node_ip: IpAddr,
    pub nodes: HashMap<IpAddr, String>,
}

impl Deployment {
    pub fn networking(&self) -> bool {
        !self.networks.is_empty()
    }

    pub fn extend_networks(&mut self, networks: Vec<Network>) -> Result<(), Error> {
        let networks = networks
            .into_iter()
            .map(|net| {
                let id = net.id.clone();
                let network = to_net(&net.ip, net.mask)?;
                let node_ip = IpAddr::from_str(&net.node_ip)?;
                let nodes = Self::map_nodes(net.nodes)?;
                Ok((
                    id,
                    DeploymentNetwork {
                        network,
                        node_ip,
                        nodes,
                    },
                ))
            })
            .collect::<Result<Vec<_>, NetError>>()?;
        self.networks.extend(networks.into_iter());
        Ok(())
    }

    pub fn map_nodes(nodes: HashMap<String, String>) -> Result<HashMap<IpAddr, String>, NetError> {
        nodes
            .into_iter()
            .map(|(ip, id)| to_ip(ip.as_ref()).map(|ip| (ip, id)))
            .collect()
    }
}
