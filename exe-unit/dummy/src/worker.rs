use super::{
    state::{State, StateMachine, Transition},
    Error, Result,
};
use actix::prelude::*;
use futures::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::time::Delay;
use futures::FutureExt;


#[derive(Default)]
pub struct Worker {
    states: StateMachine,
}

impl Actor for Worker {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "()")]
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
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        addr.send(UpdateState { state }).await?;
                        Ok((state, "".to_owned()))
                    }.left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            Command::Start { args } => {
                let transition = Transition::Start;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(Transition::Start) {
                    let addr = ctx.address().clone();
                    async move {
                        tokio::time::delay_for(Duration::from_secs(2)).await;
                        let r = addr.send(UpdateState { state }).await?;
                        Ok((state, format!("args={{{}}}", args.join(","))))
                    }.left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            Command::Run { entry_point, args } => {
                let transition = Transition::Run;
                let state = self.states.current_state;
                if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(3);
                    ActorResponse::r#async(async move {
                        tokio::time::delay_until(when.into()).await;
                        Ok((
                            state,
                            format!(
                                "entry_point={},args={{{}}}",
                                entry_point,
                                args.join(",")
                            ),
                        ))
                    }.into_actor(self))
                } else {
                    ActorResponse::reply(Err(Error::InvalidTransition { transition, state }))
                }
            }
            Command::Transfer { from, to } => {
                let transition = Transition::Transfer;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(3);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        let _ = addr.send(UpdateState { state }).await?;
                        Ok((state, format!("from={},to={}", from, to)))
                    }.left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            Command::Stop {} => {
                let transition = Transition::Transfer;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(2);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        let _r = addr.send(UpdateState { state }).await?;
                        Ok((state, "".to_owned()))
                    }.left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
        }
    }
}
