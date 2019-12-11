use anyhow::{bail, Result};
use api::{Command, Context};
use futures::{future::BoxFuture, lock::Mutex};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::time::delay_for;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum State {
    Init,
    Deployed,
    Active,
    Terminated,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum Transition {
    Deploy,
    Start,
    Run,
    Stop,
    Transfer,
}

#[derive(Debug)]
struct StateMachine {
    current_state: State,
    table: HashMap<(State, Transition), State>,
}

impl StateMachine {
    fn new() -> Self {
        // Set the start state.
        let current_state = State::Init;
        // The transition table; we let it be incomplete --
        // if the transition doesn't exist, we simply state in
        // the current state. One caveat of this approach is
        // that we lose finer error control and propagation.
        // TODO refactor state transition table
        let mut table = HashMap::new();
        table.insert((State::Init, Transition::Deploy), State::Deployed);
        table.insert((State::Deployed, Transition::Start), State::Active);
        table.insert((State::Active, Transition::Run), State::Active);
        table.insert((State::Active, Transition::Transfer), State::Active);
        table.insert((State::Active, Transition::Stop), State::Terminated);
        table.insert((State::Terminated, Transition::Transfer), State::Terminated);

        Self {
            current_state,
            table,
        }
    }

    fn next_state(&self, transition: Transition) -> Option<State> {
        self.table
            .get(&(self.current_state, transition))
            .map(|&x| x)
    }
}

#[derive(Clone)]
pub struct DummyExeUnit {
    inner: Arc<Mutex<StateMachine>>,
}

impl DummyExeUnit {
    pub fn spawn() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StateMachine::new())),
        }
    }
}

impl Context for DummyExeUnit {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DummyCmd {
    Deploy { params: Vec<String> },
    Start { params: Vec<String> },
    Run { cmd: String },
    Transfer { from: String, to: String },
    Stop {},
}

macro_rules! impl_dummy_cmd {
    (
        $name:ident => transition on ($transition:path; $($param:ident: $param_ty:ty),*) {
            $($body:tt)*
        }
    ) => {
        async fn $name(ctx: DummyExeUnit, $($param: $param_ty),*) -> Result<State> {
            let mut inner = match ctx.inner.try_lock() {
                None => bail!("couldn't acquire lock; another Op in progress"),
                Some(inner) => inner,
            };
            match inner.next_state($transition) {
                None => bail!(
                    "transition {:?} from {:?} is invalid",
                    $transition,
                    inner.current_state,
                ),
                Some(state) => {
                    $($body)*
                    inner.current_state = state;
                    Ok(state)
                }
            }
        }
    };
}

impl DummyCmd {
    // TODO the logic for state transitioning and Mutex locking is common;
    // find a way to refactor and create a re-usable API
    impl_dummy_cmd! { deploy => transition on (Transition::Deploy; _params: Vec<String>) {
        delay_for(Duration::from_secs(5)).await;
    } }

    impl_dummy_cmd! { start => transition on (Transition::Start; _params: Vec<String>) {
        delay_for(Duration::from_secs(2)).await;
    } }

    impl_dummy_cmd! { run => transition on (Transition::Run; _cmd: String) {
        delay_for(Duration::from_secs(3)).await;
    } }

    impl_dummy_cmd! { transfer => transition on (Transition::Transfer; _from: String, _to: String) {
        delay_for(Duration::from_secs(3)).await;
    } }

    impl_dummy_cmd! { stop => transition on (Transition::Stop;) {
        delay_for(Duration::from_secs(2)).await;
    } }
}

impl Command<DummyExeUnit> for DummyCmd {
    type Response = Result<State>;

    fn action(self, ctx: DummyExeUnit) -> BoxFuture<'static, Self::Response> {
        match self {
            Self::Deploy { params } => Box::pin(Self::deploy(ctx, params)),
            Self::Start { params } => Box::pin(Self::start(ctx, params)),
            Self::Run { cmd } => Box::pin(Self::run(ctx, cmd)),
            Self::Transfer { from, to } => Box::pin(Self::transfer(ctx, from, to)),
            Self::Stop {} => Box::pin(Self::stop(ctx)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table() {
        let mut state_machine = StateMachine::new();
        // From State::Init, only Transition::Deploy is valid
        assert!(state_machine.next_state(Transition::Start).is_none());
        assert!(state_machine.next_state(Transition::Run).is_none());
        assert!(state_machine.next_state(Transition::Transfer).is_none());
        assert!(state_machine.next_state(Transition::Stop).is_none());
        assert_eq!(
            state_machine.next_state(Transition::Deploy),
            Some(State::Deployed)
        );

        state_machine.current_state = State::Deployed;
        // From State::Deployed, only Transition::Start is valid
        assert!(state_machine.next_state(Transition::Deploy).is_none());
        assert!(state_machine.next_state(Transition::Run).is_none());
        assert!(state_machine.next_state(Transition::Transfer).is_none());
        assert!(state_machine.next_state(Transition::Stop).is_none());
        assert_eq!(
            state_machine.next_state(Transition::Start),
            Some(State::Active)
        );

        state_machine.current_state = State::Active;
        // From State::Active, only Transition::Run, Transition::Transfer,
        // Transition::Stop are valid
        assert!(state_machine.next_state(Transition::Deploy).is_none());
        assert!(state_machine.next_state(Transition::Start).is_none());
        assert_eq!(
            state_machine.next_state(Transition::Run),
            Some(State::Active)
        );
        assert_eq!(
            state_machine.next_state(Transition::Transfer),
            Some(State::Active)
        );
        assert_eq!(
            state_machine.next_state(Transition::Stop),
            Some(State::Terminated)
        );

        state_machine.current_state = State::Terminated;
        // From State::Terminated, only Transition::Transfer is valid
        assert!(state_machine.next_state(Transition::Deploy).is_none());
        assert!(state_machine.next_state(Transition::Start).is_none());
        assert!(state_machine.next_state(Transition::Run).is_none());
        assert!(state_machine.next_state(Transition::Stop).is_none());
        assert_eq!(
            state_machine.next_state(Transition::Transfer),
            Some(State::Terminated)
        );
    }

    #[tokio::test]
    async fn locking_inbetween_states() {
        use api::Handle;
        use futures::future::{select, FutureExt};

        let mut unit = DummyExeUnit::spawn();
        let mut unit2 = unit.clone();
        let t1 =
            tokio::spawn(async move { unit.handle(DummyCmd::Deploy { params: vec![] }).await });
        let t2 =
            tokio::spawn(async move { unit2.handle(DummyCmd::Deploy { params: vec![] }).await });
        let mut results = Vec::new();
        let res = select(t1, t2).then(|either| {
            let (res, either) = either.factor_first();
            results.push(res.unwrap().unwrap());
            either
        });
        let res = res.await.unwrap().unwrap();
        results.push(res);
        assert_eq!(results.len(), 2);
        assert_eq!(
            format!("{}", results[0].as_ref().unwrap_err()),
            "couldn't acquire lock; another Op in progress",
        );
        assert_eq!(results[1].as_ref().unwrap(), &State::Deployed);
    }

    #[tokio::test]
    async fn json_cmds() {
        use api::Exec;
        use futures::stream::StreamExt;

        let json = r#"
[
	{"deploy": { "params": [] }},
	{"start": { "params": [] }},
	{"run": { "cmd": "" }},
	{"transfer": {"from": "dumy-src", "to": "dummy-dst"}},
    {"stop": {}},
	{"transfer": {"from": "dumy-src", "to": "dummy-dst"}}
]
        "#;

        let mut unit = DummyExeUnit::spawn();
        let mut stream = <DummyExeUnit as Exec<DummyCmd>>::exec(&mut unit, json.to_string());
        let state = stream.next().await.unwrap().unwrap();
        assert_eq!(state.unwrap(), State::Deployed);
        let state = stream.next().await.unwrap().unwrap();
        assert_eq!(state.unwrap(), State::Active);
        let state = stream.next().await.unwrap().unwrap();
        assert_eq!(state.unwrap(), State::Active);
        let state = stream.next().await.unwrap().unwrap();
        assert_eq!(state.unwrap(), State::Active);
        let state = stream.next().await.unwrap().unwrap();
        assert_eq!(state.unwrap(), State::Terminated);
        let state = stream.next().await.unwrap().unwrap();
        assert_eq!(state.unwrap(), State::Terminated);
    }
}
