use anyhow::{anyhow, Result};
use derive_more::Display;
use std::collections::HashMap;
use std::fmt;
use thiserror;
use tokio::sync::watch;

use crate::market::termination_reason::BreakReason;

// =========================================== //
// Agreement state
// =========================================== //

#[derive(thiserror::Error, Clone, Debug)]
pub enum StateError {
    #[error("State for agreement [{agreement_id}] doesn't exist.")]
    NoAgreement { agreement_id: String },
    #[error(
        "Agreement [{agreement_id}] state change from {current_state} to {new_state} not allowed."
    )]
    InvalidTransition {
        agreement_id: String,
        current_state: Transition,
        new_state: AgreementState,
    },
    #[error("Failed to notify about state change to {new_state} for agreement [{agreement_id}]. Should not happen!")]
    FailedNotify {
        agreement_id: String,
        new_state: Transition,
    },
}

#[derive(Clone, Display, Debug, PartialEq)]
pub enum AgreementState {
    /// We got agreement from market.
    New,
    /// Runner and payments got agreement.
    Initialized,
    /// First activity arrived
    Computing,
    /// No active Activities.
    Idle,
    /// Requestor closed agreement satisfied.
    Closed,
    /// Provider broke agreement.
    #[display(fmt = "Broken (reason = {})", reason)]
    Broken { reason: BreakReason },
}

/// First element represents current state.
/// Second represents transition to another state or None in case, we are
/// in stable state at this moment.
#[derive(Clone, Debug)]
pub struct Transition(AgreementState, Option<AgreementState>);

#[derive(Clone)]
pub enum StateChange {
    TransitionStarted(Transition),
    TransitionFinished(AgreementState),
}

/// Helper structure allows to await for state change.
pub struct StateWaiter {
    changed_receiver: watch::Receiver<StateChange>,
}

/// Responsible for state of single tasks.
struct TaskState {
    agreement_id: String,
    state: Transition,

    changed_sender: watch::Sender<StateChange>,
    changed_receiver: watch::Receiver<StateChange>,
}

/// Responsibility: Managing agreements states changes.
pub struct TasksStates {
    tasks: HashMap<String, TaskState>,
}

impl TaskState {
    pub fn new(agreement_id: &str) -> TaskState {
        let (sender, receiver) =
            watch::channel(StateChange::TransitionFinished(AgreementState::New));
        TaskState {
            state: Transition(AgreementState::New, None),
            agreement_id: agreement_id.to_string(),
            changed_sender: sender,
            changed_receiver: receiver,
        }
    }

    pub fn allowed_transition(&self, new_state: &AgreementState) -> Result<(), StateError> {
        let is_allowed = match self.state {
            Transition(_, Some(AgreementState::Broken { .. })) => false,
            // TODO: Consider what to do when payment wasn't accepted.
            Transition(_, Some(AgreementState::Closed)) => false,
            Transition(AgreementState::New, None) => match new_state {
                AgreementState::Initialized
                | AgreementState::Broken { .. }
                | AgreementState::Closed => true,
                _ => false,
            },
            Transition(AgreementState::Initialized, None) => match new_state {
                AgreementState::Computing
                | AgreementState::Broken { .. }
                | AgreementState::Closed => true,
                _ => false,
            },
            Transition(AgreementState::Computing, None) => match new_state {
                AgreementState::Idle | AgreementState::Broken { .. } | AgreementState::Closed => {
                    true
                }
                _ => false,
            },
            Transition(AgreementState::Idle, None) => match new_state {
                AgreementState::Computing
                | AgreementState::Broken { .. }
                | AgreementState::Closed => true,
                _ => false,
            },
            Transition(_, Some(_)) => match new_state {
                AgreementState::Broken { .. } => true,
                _ => false,
            },
            _ => false,
        };

        match is_allowed {
            true => Ok(()),
            false => Err(StateError::InvalidTransition {
                agreement_id: self.agreement_id.clone(),
                current_state: self.state.clone(),
                new_state: new_state.clone(),
            }),
        }
    }

    pub fn start_transition(&mut self, new_state: AgreementState) -> Result<(), StateError> {
        self.allowed_transition(&new_state)?;
        self.state = Transition(self.state.0.clone(), Some(new_state.clone()));

        self.changed_sender
            .send(StateChange::TransitionStarted(self.state.clone()))
            .map_err(|_| StateError::FailedNotify {
                agreement_id: self.agreement_id.clone(),
                new_state: self.state.clone(),
            })?;
        Ok(())
    }

    pub fn finish_transition(&mut self, new_state: AgreementState) -> Result<(), StateError> {
        if self.state.1.as_ref() == Some(&new_state) {
            self.state = Transition(new_state.clone(), None);

            self.changed_sender
                .send(StateChange::TransitionFinished(new_state))
                .map_err(|_| StateError::FailedNotify {
                    agreement_id: self.agreement_id.clone(),
                    new_state: self.state.clone(),
                })?;
            Ok(())
        } else {
            return Err(StateError::InvalidTransition {
                agreement_id: self.agreement_id.to_string(),
                current_state: self.state.clone(),
                new_state,
            });
        }
    }

    pub fn listen_state_changes(&self) -> StateWaiter {
        StateWaiter {
            changed_receiver: self.changed_receiver.clone(),
        }
    }
}

impl TasksStates {
    pub fn new() -> TasksStates {
        TasksStates {
            tasks: HashMap::new(),
        }
    }

    pub fn new_agreement(&mut self, agreement_id: &str) -> Result<()> {
        if self.tasks.contains_key(agreement_id) {
            return Err(anyhow!(
                "TaskManager: Agreement [{}] already existed.",
                agreement_id
            ));
        }
        self.tasks
            .insert(agreement_id.to_string(), TaskState::new(agreement_id));
        Ok(())
    }

    /// Agreement is finalized or is during finalizing.
    pub fn is_agreement_finalized(&self, agreement_id: &str) -> bool {
        if let Ok(task_state) = self.get_state(agreement_id) {
            match task_state.state {
                Transition(AgreementState::Closed, _) => true,
                Transition(_, Some(AgreementState::Closed)) => true,
                Transition(AgreementState::Broken { .. }, _) => true,
                Transition(_, Some(AgreementState::Broken { .. })) => true,
                _ => false,
            }
        } else {
            false
        }
    }

    /// No Activity has been created for this Agreement
    pub fn not_active(&self, agreement_id: &str) -> bool {
        if let Ok(task_state) = self.get_state(agreement_id) {
            match task_state.state {
                Transition(AgreementState::New, _) => true,
                Transition(AgreementState::Initialized, None) => true,
                Transition(AgreementState::Idle, None) => true,
                _ => false,
            }
        } else {
            false
        }
    }

    pub fn allowed_transition(
        &self,
        agreement_id: &str,
        new_state: &AgreementState,
    ) -> Result<(), StateError> {
        let task_state = self.get_state(agreement_id)?;
        task_state.allowed_transition(new_state)
    }

    pub fn start_transition(
        &mut self,
        agreement_id: &str,
        new_state: AgreementState,
    ) -> Result<(), StateError> {
        let state = self.get_mut_state(agreement_id)?;
        state.start_transition(new_state)
    }

    pub fn finish_transition(
        &mut self,
        agreement_id: &str,
        new_state: AgreementState,
    ) -> Result<(), StateError> {
        let state = self.get_mut_state(agreement_id)?;
        state.finish_transition(new_state)
    }

    pub fn changes_listener(&self, agreement_id: &str) -> anyhow::Result<StateWaiter> {
        let state = self.get_state(agreement_id)?;
        Ok(state.listen_state_changes())
    }

    fn get_mut_state(&mut self, agreement_id: &str) -> Result<&mut TaskState, StateError> {
        match self.tasks.get_mut(agreement_id) {
            Some(state) => Ok(state),
            None => Err(StateError::NoAgreement {
                agreement_id: agreement_id.to_string(),
            }),
        }
    }

    fn get_state(&self, agreement_id: &str) -> Result<&TaskState, StateError> {
        match self.tasks.get(agreement_id) {
            Some(state) => Ok(state),
            None => Err(StateError::NoAgreement {
                agreement_id: agreement_id.to_string(),
            }),
        }
    }
}

impl StateWaiter {
    /// Returns final state of Agreement.
    pub async fn transition_finished(&mut self) -> anyhow::Result<AgreementState> {
        while let Some(change) = self
            .changed_receiver
            .changed()
            .await
            .map(|_| self.changed_receiver.borrow().clone())
            .ok()
        {
            match change {
                StateChange::TransitionFinished(state) => return Ok(state),
                _ => (),
            }
        }
        Err(anyhow!("Stopped waiting for transition finish."))
    }
}

impl fmt::Display for Transition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.1 {
            Some(state) => write!(f, "({}, {})", self.0, state),
            None => write!(f, "({}, None)", self.0),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::tasks::task_state::{AgreementState, BreakReason};

    #[test]
    #[ignore]
    fn test_state_broken_display() {
        println!(
            "{}",
            AgreementState::Broken {
                reason: BreakReason::NoActivity(chrono::Duration::seconds(17).to_std().unwrap())
            }
        );

        println!(
            "{}",
            AgreementState::Broken {
                reason: BreakReason::Expired(chrono::Utc::now())
            }
        );

        println!(
            "{}",
            AgreementState::Broken {
                reason: BreakReason::InitializationError {
                    error: "some err".to_string()
                }
            }
        )
    }
}
