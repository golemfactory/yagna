use super::{
    state::{StateMachine, Transition},
    Error, Result,
};
use actix::prelude::*;
use futures::prelude::*;
use std::time::{Duration, Instant};

pub use crate::model::*;
use futures::{FutureExt, TryFutureExt};
use std::collections::HashMap;
use std::pin::Pin;
use ya_core_model::activity::*;
use ya_model::activity::*;
use ya_service_api::constants::ACTIVITY_SERVICE_ID;
use ya_service_bus::{RpcEndpoint, RpcEnvelope, RpcMessage};

#[inline(always)]
fn invalid_service_id<R>(service_id: &String) -> std::result::Result<R, RpcMessageError> {
    Err(RpcMessageError::BadRequest(format!(
        "Invalid service id {}",
        service_id
    )))
}

pub struct Worker {
    states: StateMachine,
    service_id: Option<String>,
    actor: Option<Addr<Self>>,
    report_handle: Option<SpawnHandle>,
    batches: HashMap<String, Vec<ExeScriptCommandResult>>,
    running_command: Option<Command>,
}

impl Worker {
    fn inner_new(service_id: Option<String>) -> Self {
        Self {
            states: StateMachine::default(),
            service_id,
            actor: None,
            report_handle: None,
            batches: HashMap::new(),
            running_command: None,
        }
    }

    pub fn new(service_id: &String) -> Self {
        Self::inner_new(Some(service_id.clone()))
    }
}

impl Default for Worker {
    fn default() -> Self {
        Self::inner_new(None)
    }
}

impl Worker {
    async fn rpc<M: RpcMessage + Unpin>(uri: &str, msg: M) -> Result<<M as RpcMessage>::Item> {
        ya_service_bus::typed::service(uri)
            .call(msg)
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

        Self::rpc(ACTIVITY_SERVICE_ID, set_state).await.map(|_| ())
    }

    async fn report_usage(service_id: String, usage: ActivityUsage) {
        let set_usage = SetActivityUsage {
            activity_id: service_id,
            usage,
            timeout: Some(Duration::from_secs(1).as_millis() as u32),
        };

        if let Err(e) = Self::rpc(ACTIVITY_SERVICE_ID, set_usage).await {
            eprintln!("Error reporting usage: {:?}", e);
        }
    }

    fn start_reporting(ctx: &mut <Self as Actor>::Context, service_id: String) -> SpawnHandle {
        ctx.run_interval(Duration::from_secs(1), move |act, ctx| {
            let address = ctx.address().clone();
            let service_id = service_id.clone();
            let get_usage = RpcEnvelope::local(GetActivityUsage {
                activity_id: service_id.clone(),
                timeout: None,
            });

            let fut = async move {
                match address.send(get_usage).await {
                    Ok(result) => match result {
                        Ok(usage) => Self::report_usage(service_id.clone(), usage).await,
                        Err(error) => eprintln!("Unable to retrieve usage: {:?}", error),
                    },
                    Err(error) => eprintln!("Unable to retrieve usage: {:?}", error),
                }
            };
            ctx.spawn(fut.into_actor(act));
        })
    }
}

impl Actor for Worker {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.actor = Some(ctx.address());
        if let Some(ref service_id) = self.service_id {
            self.report_handle = Some(Self::start_reporting(ctx, service_id.clone()));
        }
    }
}

impl Handler<RpcEnvelope<GetActivityState>> for Worker {
    type Result = std::result::Result<
        <GetActivityState as RpcMessage>::Item,
        <GetActivityState as RpcMessage>::Error,
    >;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetActivityState>,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        if !self.service_id.inner_eq(&msg.activity_id) {
            return invalid_service_id(&msg.activity_id);
        }

        Ok(ActivityState {
            state: self.states.current_state,
            reason: None,
            error_message: None,
        })
    }
}

impl Handler<RpcEnvelope<GetActivityUsage>> for Worker {
    type Result = std::result::Result<
        <GetActivityUsage as RpcMessage>::Item,
        <GetActivityUsage as RpcMessage>::Error,
    >;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetActivityUsage>,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        if !self.service_id.inner_eq(&msg.activity_id) {
            return invalid_service_id(&msg.activity_id);
        }

        Ok(ActivityUsage {
            current_usage: None,
        })
    }
}

impl Handler<RpcEnvelope<Exec>> for Worker {
    type Result = std::result::Result<<Exec as RpcMessage>::Item, <Exec as RpcMessage>::Error>;

    fn handle(&mut self, msg: RpcEnvelope<Exec>, _ctx: &mut Self::Context) -> Self::Result {
        if !self.service_id.inner_eq(&msg.activity_id) {
            return invalid_service_id(&msg.activity_id);
        }

        self.batches.remove(&msg.batch_id);

        let actor = self.actor.clone().unwrap();
        let batch_id = msg.batch_id.clone();
        let batch_idx = msg.batch_id.clone();

        let fut: Pin<Box<dyn Future<Output = Result<()>> + Send>> = async move {
            for (index, cmd) in msg.exe_script.iter().enumerate() {
                let cmd_result = actor.send(Command(cmd.clone())).await?;
                let (result, message) = match cmd_result {
                    Ok((_, message)) => (CommandResult::Ok, message),
                    Err(error) => (CommandResult::Error, format!("{:?}", error)),
                };
                let update = UpdateBatchResult {
                    batch_id: batch_idx.clone(),
                    result: ExeScriptCommandResult {
                        index: index as u32,
                        result: Some(result),
                        message: Some(message),
                    },
                };

                actor.send(update).await?;
                if result == CommandResult::Error {
                    break;
                }
            }
            Ok(())
        }
        .boxed();

        ActorResponse::r#async(fut.into_actor(self));
        Ok(batch_id)
    }
}

impl Handler<RpcEnvelope<GetRunningCommand>> for Worker {
    type Result = std::result::Result<
        <GetRunningCommand as RpcMessage>::Item,
        <GetRunningCommand as RpcMessage>::Error,
    >;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetRunningCommand>,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        if !self.service_id.inner_eq(&msg.activity_id) {
            return invalid_service_id(&msg.activity_id);
        }

        let state = match &self.running_command {
            Some(cmd) => ExeScriptCommandState {
                command: serde_json::to_string(&cmd.0).unwrap(),
                progress: None,
                params: None,
            },
            None => ExeScriptCommandState {
                command: "".to_string(),
                progress: None,
                params: None,
            },
        };

        Ok(state)
    }
}

impl Handler<RpcEnvelope<GetExecBatchResults>> for Worker {
    type Result = std::result::Result<
        <GetExecBatchResults as RpcMessage>::Item,
        <GetExecBatchResults as RpcMessage>::Error,
    >;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetExecBatchResults>,
        _ctx: &mut Self::Context,
    ) -> Self::Result {
        if !self.service_id.inner_eq(&msg.activity_id) {
            return invalid_service_id(&msg.activity_id);
        }

        let results = match self.batches.get(&msg.batch_id) {
            Some(results) => results.clone(),
            None => Vec::new(),
        };
        Ok(results)
    }
}

impl Handler<UpdateBatchResult> for Worker {
    type Result = ();

    fn handle(&mut self, msg: UpdateBatchResult, _ctx: &mut Self::Context) -> Self::Result {
        match self.batches.get_mut(&msg.batch_id) {
            Some(vec) => vec.push(msg.result),
            None => {
                self.batches.insert(msg.batch_id, vec![msg.result]);
            }
        }
    }
}

impl Handler<UpdateRunningCommand> for Worker {
    type Result = ();

    fn handle(&mut self, msg: UpdateRunningCommand, _ctx: &mut Self::Context) -> Self::Result {
        self.running_command = msg.0;
    }
}

impl Handler<UpdateState> for Worker {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: UpdateState, ctx: &mut Self::Context) -> Self::Result {
        self.states.current_state = msg.state;
        if self.states.current_state == State::Terminated {
            if let Some(handle) = self.report_handle.take() {
                ctx.cancel_future(handle);
            }
        }

        match self.service_id {
            Some(ref service_id) => {
                let fut = Self::report_state(service_id.clone(), self.states.current_state);
                ActorResponse::r#async(fut.into_actor(self))
            }
            None => ActorResponse::reply(Ok(())),
        }
    }
}

impl Handler<Command> for Worker {
    type Result = ActorResponse<Self, (State, String), Error>;

    fn handle(&mut self, msg: Command, ctx: &mut Self::Context) -> Self::Result {
        let address = ctx.address().clone();
        let command = msg.clone();
        let fut = match msg.0 {
            ExeScriptCommand::Deploy {} => {
                let transition = Transition::Deploy;
                let state = self.states.current_state;

                if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(5);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        addr.send(UpdateState { state }).await??;
                        Ok((state, "".to_owned()))
                    }
                    .boxed()
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                }
            }
            ExeScriptCommand::Start { args } => {
                let transition = Transition::Start;
                let state = self.states.current_state;

                if let Some(state) = self.states.next_state(Transition::Start) {
                    let addr = ctx.address().clone();
                    async move {
                        tokio::time::delay_for(Duration::from_secs(2)).await;
                        let _r = addr.send(UpdateState { state }).await?;
                        Ok((state, format!("args={{{}}}", args.join(","))))
                    }
                    .boxed()
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                }
            }
            ExeScriptCommand::Run { entry_point, args } => {
                let transition = Transition::Run;
                let state = self.states.current_state;

                if let Some(state) = self.states.next_state(transition) {
                    let _addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(3);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        Ok((
                            state,
                            format!("entry_point={},args={{{}}}", entry_point, args.join(",")),
                        ))
                    }
                    .boxed()
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                }
            }
            ExeScriptCommand::Transfer { from, to } => {
                let transition = Transition::Transfer;
                let state = self.states.current_state;

                if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(3);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        let _ = addr.send(UpdateState { state }).await?;
                        Ok((state, format!("from={},to={}", from, to)))
                    }
                    .boxed()
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                }
            }
            ExeScriptCommand::Stop {} => {
                let transition = Transition::Stop;
                let state = self.states.current_state;

                if let Some(state) = self.states.next_state(transition) {
                    let addr = ctx.address().clone();
                    let when = Instant::now() + Duration::from_secs(2);
                    async move {
                        tokio::time::delay_until(when.into()).await;
                        let _r = addr.send(UpdateState { state }).await?;
                        Ok((state, "".to_owned()))
                    }
                    .boxed()
                    .left_future()
                } else {
                    future::err(Error::InvalidTransition { transition, state }).right_future()
                }
            }
        };

        let fut = async move {
            address.send(UpdateRunningCommand(Some(command))).await?;
            fut.await
        };
        ActorResponse::r#async(fut.into_actor(self))
    }
}
