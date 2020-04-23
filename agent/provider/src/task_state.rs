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

#[derive(Clone, Display, Debug)]
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

/// Responsibility: Managing agreements states changes.
pub struct TaskStates {
    tasks: HashMap<String, Transition>,
}

impl TaskStates {
    pub fn new() -> TaskStates {
        TaskStates {
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
        self.tasks.insert(
            agreement_id.to_string(),
            Transition(AgreementState::New, None),
        );
        Ok(())
    }

    pub fn allowed_transition(
        &self,
        agreement_id: &str,
        new_state: &AgreementState,
    ) -> Result<(), StateError> {
        let state = self
            .tasks
            .get(agreement_id)
            .ok_or(StateError::NoAgreement {
                agreement_id: agreement_id.to_string(),
            })?;

        let is_allowed = match state {
            Transition(AgreementState::New, _) => match new_state {
                AgreementState::Initialized
                | AgreementState::Broken { .. }
                | AgreementState::Closed => true,
                _ => false,
            },
            Transition(AgreementState::Initialized, _) => match new_state {
                AgreementState::Computing
                | AgreementState::Broken { .. }
                | AgreementState::Closed => true,
                _ => false,
            },
            Transition(AgreementState::Computing, _) => match new_state {
                AgreementState::Computing
                | AgreementState::Broken { .. }
                | AgreementState::Closed => true,
                _ => false,
            },
            Transition(AgreementState::Broken { .. }, _) => match new_state {
                AgreementState::Broken { .. } => true,
                _ => false,
            },
            Transition(AgreementState::Closed, _) => match new_state {
                AgreementState::Closed => true,
                _ => false,
            },
        };

        match is_allowed {
            true => Ok(()),
            false => Err(StateError::InvalidTransition {
                agreement_id: agreement_id.to_string(),
                current_state: state.clone(),
                new_state: new_state.clone(),
            }),
        }
    }

    pub fn start_transition(
        &mut self,
        agreement_id: &str,
        new_state: AgreementState,
    ) -> Result<(), StateError> {
        self.allowed_transition(agreement_id, &new_state)?;
        self.tasks
            .entry(agreement_id.to_string())
            .and_modify(|state| *state = Transition(state.0.clone(), Some(new_state)));
        Ok(())
    }

    pub fn finish_transition(
        &mut self,
        agreement_id: &str,
        new_state: AgreementState,
    ) -> Result<(), StateError> {
        self.allowed_transition(agreement_id, &new_state)?;
        self.tasks
            .entry(agreement_id.to_string())
            .and_modify(|state| *state = Transition(new_state, None));
        Ok(())
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
