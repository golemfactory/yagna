use crate::error::Error;
use crate::message::{GetBatchResults, GetMetrics};
use crate::runtime::Runtime;
use crate::ExeUnit;
use actix::prelude::*;
use chrono::Utc;
use futures::channel::oneshot;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::time::timeout;
use ya_client_model::activity::{ActivityState, ActivityUsage, ExeScriptCommandResult};
use ya_core_model::activity::*;
use ya_service_bus::{Error as RpcError, RpcEnvelope, RpcStreamCall};

impl<R: Runtime> Handler<RpcEnvelope<Exec>> for ExeUnit<R> {
    type Result = <RpcEnvelope<Exec> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<Exec>, ctx: &mut Self::Context) -> Self::Result {
        self.ctx.verify_activity_id(&msg.activity_id)?;

        let batch_id = msg.batch_id.clone();
        if self.state.batches.contains_key(&batch_id) {
            let m = format!("Batch {} already exists", batch_id);
            return Err(RpcMessageError::BadRequest(m));
        }

        let (tx, rx) = oneshot::channel();
        let msg = msg.into_inner();
        self.state.start_batch(msg.clone(), tx);

        let fut = Self::exec(
            msg,
            ctx.address(),
            self.runtime.clone(),
            self.transfers.clone(),
            self.events.tx.clone(),
            rx,
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
            return ActorResponse::reply(Err(e.into()));
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
                    timestamp: Utc::now().timestamp(),
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

        // FIXME: use batch_id when added to GetRunningCommand
        if let Some(recent) = self.state.last_batch.clone() {
            if let Some(batch) = self.state.batches.get(&recent) {
                if let Some(cmd) = batch.running_cmd() {
                    return Ok(cmd);
                }
            }
        }

        Err(RpcMessageError::NotFound(format!(
            "No running command within activity: {}",
            msg.activity_id
        )))
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

        let batch = match self.state.batches.get(&msg.batch_id) {
            Some(batch) => batch,
            None => {
                let err = RpcMessageError::NotFound(format!("batch_id = {}", msg.batch_id));
                return ActorResponse::reply(Err(err));
            }
        };
        let idx = match batch.script.exe_script.len() {
            0 => return ActorResponse::reply(Ok(Vec::new())),
            len => msg.command_index.unwrap_or(len - 1),
        };

        let address = ctx.address();
        let duration = Duration::from_secs_f32(msg.timeout.unwrap_or(0.));
        let notifier = batch.notifier.clone();

        let fut = async move {
            if let Err(_) = timeout(duration, notifier.when(move |i| i >= idx)).await {
                if msg.command_index.is_some() {
                    return Err(RpcMessageError::Timeout);
                }
            }
            match address.send(GetBatchResults(msg.batch_id.clone())).await {
                Ok(mut results) => {
                    results.0.truncate(idx + 1);
                    Ok(results.0)
                }
                _ => Ok(Vec::new()),
            }
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl<R: Runtime> Handler<RpcStreamCall<StreamExecBatchProgress>> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), RpcError>;

    fn handle(
        &mut self,
        msg: RpcStreamCall<StreamExecBatchProgress>,
        _: &mut Self::Context,
    ) -> Self::Result {
        if let Err(e) = self.ctx.verify_activity_id(&msg.body.activity_id) {
            return ActorResponse::reply(Err(RpcError::GsbBadRequest(e.to_string())));
        }
        let batch = match self.state.batches.get_mut(&msg.body.batch_id) {
            Some(batch) => batch,
            _ => {
                let msg = format!("Unknown batch: {}", msg.body.batch_id);
                return ActorResponse::reply(Err(RpcError::GsbBadRequest(msg)));
            }
        };

        let rx = batch.stream.receiver().into_stream().map(|r| match r {
            Ok(v) => Ok::<_, RpcError>(Ok(v)),
            Err(e) => Ok::<_, RpcError>(Err(RpcMessageError::Service(e.to_string()))),
        });
        let reply = msg
            .reply
            .sink_map_err(|e| RpcError::GsbFailure(e.to_string()));

        ActorResponse::r#async(async move { rx.forward(reply).await }.into_actor(self))
    }
}
