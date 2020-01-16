use super::{
    state::{StateMachine, Transition},
    Error, Result,
};
use actix::prelude::*;
use futures::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use futures::FutureExt;
use ya_core_model::activity::{Exec, SetActivityState, SetActivityUsage};
use ya_model::activity::{ActivityState, ActivityUsage, ExeScriptCommand, State};
use ya_service_bus::{RpcEndpoint, RpcMessage};

const ACTIVITY_SERVICE_GSB: &str = "/activity";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<(State, String)>")]
pub struct Command(ExeScriptCommand);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Message)]
#[rtype(result = "Result<()>")]
pub struct Commands(Exec);

impl RpcMessage for Commands {
    const ID: &'static str = <Exec as RpcMessage>::ID;
    type Item = <Exec as RpcMessage>::Item;
    type Error = <Exec as RpcMessage>::Error;
}

pub struct Worker {
    states: StateMachine,
    service_id: Option<String>,
    actor: Option<Addr<Self>>,
    report_usage_handle: Option<SpawnHandle>,
}

impl Default for Worker {
    fn default() -> Self {
        Self {
            states: StateMachine::default(),
            service_id: None,
            actor: None,
            report_usage_handle: None,
        }
    }
}

impl Worker {
    pub fn new(service_id: &String) -> Self {
        Self {
            states: StateMachine::default(),
            service_id: Some(service_id.clone()),
            actor: None,
            report_usage_handle: None,
        }
    }
}

impl Worker {
    async fn rpc<M: RpcMessage + Unpin>(uri: &str, msg: M) -> Result<<M as RpcMessage>::Item> {
        ya_service_bus::typed::service(uri)
            .send(msg)
            .map_err(Error::from)
            .await?
            .map_err(|e| Error::RemoteServiceError(format!("{:?}", e)))
    }

    async fn report_state(service_id: String, state: State) -> Result<()> {
        let set_state = SetActivityState {
            activity_id: service_id,
            state: ActivityState {
                state,
                reason: None,
                error_message: None,
            },
            timeout: Some(Duration::from_secs(1).as_millis() as u32),
        };

        Self::rpc(ACTIVITY_SERVICE_GSB, set_state).await?;
        Ok(())
    }

    async fn report_usage(service_id: String, usage: Option<Vec<f64>>) {
        let set_usage = SetActivityUsage {
            activity_id: service_id,
            usage: ActivityUsage {
                current_usage: usage,
            },
            timeout: Some(Duration::from_secs(1).as_millis() as u32),
        };

        if let Err(e) = Self::rpc(ACTIVITY_SERVICE_GSB, set_usage).await {
            eprintln!("Error while reporting usage: {:?}", e);
        }
    }

    fn start_report_usage_interval(
        &mut self,
        ctx: &mut <Self as Actor>::Context,
    ) -> Option<SpawnHandle> {
        let service_id = match self.service_id {
            Some(ref id) => id.clone(),
            None => return None,
        };

        let handle = ctx.run_interval(Duration::from_secs(1), move |act, ctx| {
            let service_id = service_id.clone();
            let address = ctx.address().clone();

            let fut = async move {
                match address.send(GetUsage::default()).await {
                    Ok(usage) => Self::report_usage(service_id.clone(), usage).await,
                    Err(error) => eprintln!("Unable to retrieve usage: {:?}", error),
                }
            };
            ctx.spawn(fut.into_actor(act));
        });

        Some(handle)
    }

    fn stop_report_usage_interval(&mut self, ctx: &mut <Self as Actor>::Context) {
        if let Some(handle) = self.report_usage_handle {
            ctx.cancel_future(handle);
            self.report_usage_handle = None;
        }
    }
}

impl Actor for Worker {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.actor = Some(ctx.address());
        self.report_usage_handle = self.start_report_usage_interval(ctx);
    }
}

#[derive(Clone, Debug, Default, Message)]
#[rtype(result = "Option<Vec<f64>>")]
struct GetUsage;

impl Handler<GetUsage> for Worker {
    type Result = Option<Vec<f64>>;

    fn handle(&mut self, _msg: GetUsage, _ctx: &mut Self::Context) -> Self::Result {
        None
    }
}

#[derive(Message)]
#[rtype(result = "Result<()>")]
struct UpdateState {
    state: State,
}

impl Handler<UpdateState> for Worker {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: UpdateState, ctx: &mut Self::Context) -> Self::Result {
        self.states.current_state = msg.state;
        if msg.state == State::Terminated {
            self.stop_report_usage_interval(ctx);
        }

        match self.service_id {
            Some(ref service_id) => {
                let state = self.states.current_state;
                let service_id = service_id.clone();
                ActorResponse::r#async(Self::report_state(service_id, state).into_actor(self))
            }
            None => ActorResponse::reply(Ok(())),
        }
    }
}

impl Handler<Commands> for Worker {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Commands, _ctx: &mut Self::Context) -> Self::Result {
        let actor = self.actor.clone().unwrap();
        let fut = async move {
            for cmd in msg.0.exe_script.into_iter() {
                actor.send(Command(cmd)).await??;
            }
            Ok(())
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl Handler<Command> for Worker {
    type Result = ActorResponse<Self, (State, String), Error>;

    fn handle(&mut self, msg: Command, ctx: &mut Self::Context) -> Self::Result {
        match msg.0 {
            ExeScriptCommand::Deploy {} => {
                let transition = Transition::Deploy;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(5);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        addr.send(UpdateState { state }).await??;
                        Ok((state, "".to_owned()))
                    }
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            ExeScriptCommand::Start { args } => {
                let transition = Transition::Start;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(Transition::Start) {
                    let addr = ctx.address().clone();
                    async move {
                        tokio::time::delay_for(Duration::from_secs(2)).await;
                        let _r = addr.send(UpdateState { state }).await?;
                        Ok((state, format!("args={{{}}}", args.join(","))))
                    }
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            ExeScriptCommand::Run { entry_point, args } => {
                let transition = Transition::Run;
                let state = self.states.current_state;
                if let Some(state) = self.states.next_state(transition) {
                    let _addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(3);
                    ActorResponse::r#async(
                        async move {
                            tokio::time::delay_until(when.into()).await;
                            Ok((
                                state,
                                format!("entry_point={},args={{{}}}", entry_point, args.join(",")),
                            ))
                        }
                        .into_actor(self),
                    )
                } else {
                    ActorResponse::reply(Err(Error::InvalidTransition { transition, state }))
                }
            }
            ExeScriptCommand::Transfer { from, to } => {
                let transition = Transition::Transfer;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(3);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        let _ = addr.send(UpdateState { state }).await?;
                        Ok((state, format!("from={},to={}", from, to)))
                    }
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
            ExeScriptCommand::Stop {} => {
                let transition = Transition::Transfer;
                let state = self.states.current_state;
                let fut = if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(2);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        let _r = addr.send(UpdateState { state }).await?;
                        Ok((state, "".to_owned()))
                    }
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                };
                ActorResponse::r#async(fut.into_actor(self))
            }
        }
    }
}
