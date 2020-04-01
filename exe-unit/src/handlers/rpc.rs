use crate::error::Error;
use crate::message::{GetBatchResults, GetMetrics};
use crate::runtime::Runtime;
use crate::ExeUnit;
use actix::prelude::*;
use futures::{FutureExt, TryFutureExt};
use std::time::Duration;
use ya_core_model::activity::*;
use ya_model::activity::{ActivityState, ActivityUsage, ExeScriptCommandResult};
use ya_service_bus::timeout::IntoTimeoutFuture;
use ya_service_bus::RpcEnvelope;

impl<R: Runtime> Handler<RpcEnvelope<Exec>> for ExeUnit<R> {
    type Result = <RpcEnvelope<Exec> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<Exec>, ctx: &mut Self::Context) -> Self::Result {
        self.ctx.verify_activity_id(&msg.activity_id)?;
        self.state.batches.insert(msg.batch_id.clone(), msg.clone());

        let batch_id = msg.batch_id.clone();
        let fut = Self::exec(
            ctx.address(),
            self.runtime.clone(),
            self.transfers.clone(),
            msg.into_inner(),
        );
        ctx.spawn(fut.into_actor(self));

        Ok(batch_id)
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetState>> for ExeUnit<R> {
    type Result = <RpcEnvelope<GetState> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<GetState>, _: &mut Self::Context) -> Self::Result {
        self.ctx.verify_activity_id(&msg.activity_id)?;

        Ok(ActivityState {
            state: self.state.inner.clone(),
            reason: None,
            error_message: None,
        })
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetUsage>> for ExeUnit<R> {
    type Result = ActorResponse<Self, ActivityUsage, RpcMessageError>;

    fn handle(&mut self, msg: RpcEnvelope<GetUsage>, _: &mut Self::Context) -> Self::Result {
        if let Err(e) = self.ctx.verify_activity_id(&msg.activity_id) {
            return ActorResponse::r#async(futures::future::err(e.into()).into_actor(self));
        }

        let metrics = self.metrics.clone();
        let fut = async move {
            let resp = match metrics.send(GetMetrics).await {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("Unable to report activity usage: {:?}", e);
                    return Err(Error::from(e).into());
                }
            };

            match resp {
                Ok(data) => Ok(ActivityUsage {
                    current_usage: Some(data),
                }),
                Err(e) => Err(Error::from(e).into()),
            }
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetRunningCommand>> for ExeUnit<R> {
    type Result = <RpcEnvelope<GetRunningCommand> as Message>::Result;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetRunningCommand>,
        _: &mut Self::Context,
    ) -> Self::Result {
        self.ctx.verify_activity_id(&msg.activity_id)?;

        match &self.state.running_command {
            Some(command) => Ok(command.clone()),
            None => Err(RpcMessageError::NotFound(format!(
                "no command is running within activity id: {}",
                msg.activity_id
            ))),
        }
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetExecBatchResults>> for ExeUnit<R> {
    type Result = ActorResponse<Self, Vec<ExeScriptCommandResult>, RpcMessageError>;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetExecBatchResults>,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        if let Err(err) = self.ctx.verify_activity_id(&msg.activity_id) {
            return ActorResponse::reply(Err(err.into()));
        }

        let address = ctx.address();
        let timeout = msg.timeout.clone();
        let delay = Duration::from_millis(500);

        let last_idx = match self.state.batches.get(&msg.batch_id) {
            Some(exec) => match exec.exe_script.len() {
                0 => return ActorResponse::reply(Ok(Vec::new())),
                l => l - 1,
            },
            None => {
                let err = RpcMessageError::NotFound(format!("batch_id = {}", msg.batch_id));
                return ActorResponse::reply(Err(err));
            }
        };

        let fut = async move {
            let idx = msg.command_index.unwrap_or(last_idx) as usize;
            loop {
                if let Ok(results) = address.send(GetBatchResults(msg.batch_id.clone())).await {
                    if results.0.len() >= idx + 1 {
                        let mut results = results.0;
                        results.truncate(idx + 1);
                        break Ok(results);
                    }
                }
                tokio::time::delay_for(delay).await;
            }
        };

        ActorResponse::r#async(
            fut.timeout(timeout)
                .map_err(|_| RpcMessageError::Timeout)
                .map(|r| match r {
                    Ok(r) => r,
                    Err(e) => Err(e),
                })
                .into_actor(self),
        )
    }
}
