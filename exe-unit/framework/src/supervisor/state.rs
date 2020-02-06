use serde::Serialize;
use std::collections::HashMap;
use ya_model::activity::State;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Serialize)]
pub enum Transition {
    Deploy,
    Start,
    Run,
    Stop,
    Transfer,
}

#[derive(Debug)]
pub(super) struct StateMachine {
    pub(super) current_state: State,
    table: HashMap<(State, Transition), State>,
}

impl Default for StateMachine {
    fn default() -> Self {
        // Set the start state.
        let current_state = State::default();
        // The transition table; we let it be incomplete --
        // if the transition doesn't exist, we simply state in
        // the current state. One caveat of this approach is
        // that we lose finer error control and propagation.
        // TODO refactor state transition table
        let mut table = HashMap::new();
        table.insert((State::New, Transition::Deploy), State::Ready);
        table.insert((State::Ready, Transition::Start), State::Active);
        table.insert((State::Active, Transition::Run), State::Active);
        table.insert((State::Active, Transition::Transfer), State::Active);
        table.insert((State::Active, Transition::Stop), State::Terminated);
        table.insert((State::Terminated, Transition::Transfer), State::Terminated);

        Self {
            current_state,
            table,
        }
    }
}

impl StateMachine {
    pub(super) fn next_state(&self, transition: Transition) -> Option<State> {
        self.table
            .get(&(self.current_state, transition))
            .map(|&x| x)
    }
}
