use anyhow::{anyhow, Result};
use derive_more::Display;
use std::collections::HashMap;
use std::fmt;
use thiserror;

#[derive(Display, Debug, Clone, PartialEq)]
pub enum BreakReason {
    InitializationError { error: String },
    Expired,
}

// =========================================== //
// Agreement state
// =========================================== //

#[derive(thiserror::Error, Clone, Debug)]
pub enum StateError {
    #[error("State for agreement [{agreement_id}] doesn't exist.")]
    NoAgreement { agreement_id: String },
    #[error(
        "Agreement [{agreement_id}] state change from {current_state}, to {new_state} not allowed."
    )]
    InvalidTransition {
        agreement_id: String,
        current_state: Transition,
        new_state: AgreementState,
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
    /// Requestor closed agreement satisfied.
    Closed,
    /// Provider broke agreement.
    Broken { reason: BreakReason },
}

/// First element represents current state.
/// Second represents transition to another state or None in case, we are
/// in stable state at this moment.
#[derive(Clone, Debug)]
pub struct Transition(AgreementState, Option<AgreementState>);

/// Responsible for state of single task.
struct TaskState {
    agreement_id: String,
    state: Transition,
}

/// Responsibility: Managing agreements states changes.
pub struct TasksStates {
    tasks: HashMap<String, TaskState>,
}

impl TaskState {
    pub fn new(agreement_id: &str) -> TaskState {
        TaskState {
            state: Transition(AgreementState::New, None),
            agreement_id: agreement_id.to_string(),
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
        self.state = Transition(self.state.0.clone(), Some(new_state));
        Ok(())
    }

    pub fn finish_transition(&mut self, new_state: AgreementState) -> Result<(), StateError> {
        if self.state.1.as_ref() == Some(&new_state) {
            self.state = Transition(new_state, None);
            Ok(())
        } else {
            return Err(StateError::InvalidTransition {
                agreement_id: self.agreement_id.to_string(),
                current_state: self.state.clone(),
                new_state,
            });
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

impl fmt::Display for Transition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.1 {
            Some(state) => write!(f, "({}, {})", self.0, state),
            None => write!(f, "({}, None)", self.0),
        }
    }
}
