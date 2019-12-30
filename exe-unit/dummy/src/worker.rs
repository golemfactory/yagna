use super::{
    state::{State, StateMachine, Transition},
    Error, Result,
};
use actix::prelude::*;
use futures::future::{self, Future};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::timer::Delay;

#[derive(Default)]
pub struct Worker {
    states: StateMachine,
}

impl Actor for Worker {
    type Context = Context<Self>;
}

#[derive(Message)]
struct UpdateState {
    state: State,
}

impl Handler<UpdateState> for Worker {
    type Result = ();

    fn handle(&mut self, msg: UpdateState, _ctx: &mut Self::Context) -> Self::Result {
        self.states.current_state = msg.state;
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Command {
    Deploy {},
    Start {
        #[serde(default)]
        args: Vec<String>,
    },
    Run {
        entry_point: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Stop {},
    Transfer {
        from: String,
        to: String,
    },
}

impl Message for Command {
    type Result = Result<(State, String)>;
}

impl Handler<Command> for Worker {
    type Result = ActorResponse<Self, (State, String), Error>;

    fn handle(&mut self, msg: Command, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            Command::Deploy {} => {
                let transition = Transition::Deploy;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(5);
                    future::Either::A(Delay::new(when).map_err(Into::into).and_then(move |_| {
                        addr.send(UpdateState { state })
                            .map_err(Into::into)
                            .and_then(move |_| Ok((state, "".to_owned())))
                    }))
                } else {
                    future::Either::B(future::err(Error::InvalidTransition { transition, state }))
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            Command::Start { args } => {
                let transition = Transition::Start;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(Transition::Start) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(2);
                    future::Either::A(Delay::new(when).map_err(Into::into).and_then(move |_| {
                        addr.send(UpdateState { state })
                            .map_err(Into::into)
                            .and_then(move |_| Ok((state, format!("args={{{}}}", args.join(",")))))
                    }))
                } else {
                    future::Either::B(future::err(Error::InvalidTransition { transition, state }))
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            Command::Run { entry_point, args } => {
                let transition = Transition::Run;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(3);
                    future::Either::A(Delay::new(when).map_err(Into::into).and_then(move |_| {
                        addr.send(UpdateState { state })
                            .map_err(Into::into)
                            .and_then(move |_| {
                                Ok((
                                    state,
                                    format!(
                                        "entry_point={},args={{{}}}",
                                        entry_point,
                                        args.join(",")
                                    ),
                                ))
                            })
                    }))
                } else {
                    future::Either::B(future::err(Error::InvalidTransition { transition, state }))
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            Command::Transfer { from, to } => {
                let transition = Transition::Transfer;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(3);
                    future::Either::A(Delay::new(when).map_err(Into::into).and_then(move |_| {
                        addr.send(UpdateState { state })
                            .map_err(Into::into)
                            .and_then(move |_| Ok((state, format!("from={},to={}", from, to))))
                    }))
                } else {
                    future::Either::B(future::err(Error::InvalidTransition { transition, state }))
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            Command::Stop {} => {
                let transition = Transition::Transfer;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(2);
                    future::Either::A(Delay::new(when).map_err(Into::into).and_then(move |_| {
                        addr.send(UpdateState { state })
                            .map_err(Into::into)
                            .and_then(move |_| Ok((state, "".to_owned())))
                    }))
                } else {
                    future::Either::B(future::err(Error::InvalidTransition { transition, state }))
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
        }
    }
}
