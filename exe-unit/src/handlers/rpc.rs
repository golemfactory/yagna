use std::time::Duration;

use actix::prelude::*;
use chrono::Utc;
use futures::channel::oneshot;
use futures::{SinkExt, StreamExt};
use tokio::time::timeout;

#[cfg(feature = "sgx")]
use ya_client_model::activity::encrypted::RpcMessageError as SgxMessageError;
use ya_client_model::activity::{ActivityState, ActivityUsage, ExeScriptCommandResult};
use ya_core_model::activity::*;
use ya_service_bus::{Error as RpcError, RpcEnvelope, RpcStreamCall};

use crate::error::Error;
use crate::manifest::{ManifestValidatorExt, ScriptValidator};
use crate::message::{GetBatchResults, GetMetrics};
use crate::runtime::Runtime;
use crate::{ExeUnit, RuntimeRef};

impl<R: Runtime> Handler<RpcEnvelope<Exec>> for ExeUnit<R> {
    type Result = <RpcEnvelope<Exec> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<Exec>, ctx: &mut Self::Context) -> Self::Result {
        self.ctx.verify_activity_id(&msg.activity_id)?;

        let batch_id = msg.batch_id.clone();
        let msg = msg.into_inner();

        if self.state.batches.contains_key(&batch_id) {
            let m = format!("Batch {} already exists", batch_id);
            return Err(RpcMessageError::BadRequest(m));
        }

        let validator = self.ctx.supervise.manifest.validator::<ScriptValidator>();
        if let Err(e) = validator.with(|c| c.validate(msg.exe_script.iter())) {
            let m = format!("Manifest violation in ExeScript: {}", e);
            return Err(RpcMessageError::BadRequest(m));
        }

        let (tx, rx) = oneshot::channel();
        self.state.start_batch(msg.clone(), tx);

        RuntimeRef::from_ctx(&ctx)
            .exec(
                msg,
                self.runtime.clone(),
                self.transfers.clone(),
                self.events.tx.clone(),
                rx,
            )
            .into_actor(self)
            .spawn(ctx);

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
                Err(e) => Err(e.into()),
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
        let commands = self
            .state
            .batches
            .values()
            .filter_map(|b| b.running_command())
            .collect::<Vec<_>>();

        if !commands.is_empty() {
            return Ok(commands);
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
        let await_idx = match batch.exec.exe_script.len() {
            0 => return ActorResponse::reply(Ok(Vec::new())),
            len => msg.command_index.unwrap_or(len - 1),
        };

        let address = ctx.address();
        let duration = Duration::from_secs_f32(msg.timeout.unwrap_or(0.));
        let notifier = batch.notifier.clone();

        let idx = msg.command_index.clone();
        let batch_id = msg.batch_id.clone();

        let fut = async move {
            if timeout(duration, notifier.when(move |i| i >= await_idx))
                .await
                .is_err()
            {
                if msg.command_index.is_some() {
                    return Err(RpcMessageError::Timeout);
                }
            }
            match address.send(GetBatchResults { batch_id, idx }).await {
                Ok(results) => Ok(results.0),
                _ => Ok(Vec::new()),
            }
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl<R: Runtime> Handler<RpcStreamCall<StreamExecBatchResults>> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), RpcError>;

    fn handle(
        &mut self,
        msg: RpcStreamCall<StreamExecBatchResults>,
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

        let rx = batch.stream.receiver().map(|r| match r {
            Ok(v) => Ok::<_, RpcError>(Ok(v)),
            Err(e) => Ok::<_, RpcError>(Err(RpcMessageError::Service(e.to_string()))),
        });
        let reply = msg
            .reply
            .sink_map_err(|e| RpcError::GsbFailure(e.to_string()));

        ActorResponse::r#async(async move { rx.forward(reply).await }.into_actor(self))
    }
}

#[cfg(feature = "sgx")]
impl<R: Runtime> Handler<RpcEnvelope<sgx::CallEncryptedService>> for ExeUnit<R> {
    type Result = ResponseFuture<Result<Vec<u8>, RpcMessageError>>;

    fn handle(
        &mut self,
        msg: RpcEnvelope<sgx::CallEncryptedService>,
        ctx: &mut Context<Self>,
    ) -> Self::Result {
        use futures::prelude::*;
        use ya_client_model::activity::encrypted::{Request, RequestCommand, Response};

        let me = ctx.address();
        let dec = self.ctx.crypto.ctx();
        let enc = self.ctx.crypto.ctx();

        async move {
            let request: Request = dec
                .decrypt(msg.bytes.as_slice())
                .map_err(|e| SgxMessageError::BadRequest(format!("Decryption error: {:?}", e)))?;
            let activity_id = request.activity_id;
            let batch_id = request.batch_id;
            let timeout = request.timeout;

            Ok(match request.command {
                RequestCommand::Exec { exe_script } => {
                    let msg = Exec {
                        activity_id,
                        batch_id,
                        timeout,
                        exe_script,
                    };
                    Response::Exec(
                        me.send(RpcEnvelope::local(msg))
                            .await
                            .map_err(|_| {
                                SgxMessageError::Service("fatal: exe-unit disconnected".to_string())
                            })?
                            .map_err(rpc_to_sgx_error),
                    )
                }
                RequestCommand::GetExecBatchResults { command_index } => {
                    let msg = GetExecBatchResults {
                        activity_id,
                        batch_id,
                        timeout,
                        command_index,
                    };
                    Response::GetExecBatchResults(
                        me.send(RpcEnvelope::local(msg))
                            .await
                            .map_err(|_e| {
                                SgxMessageError::Service("fatal: exe-unit disconnected".to_string())
                            })?
                            .map_err(rpc_to_sgx_error),
                    )
                }
                RequestCommand::GetRunningCommand => {
                    let msg = GetRunningCommand {
                        activity_id,
                        timeout,
                    };
                    Response::GetRunningCommand(
                        me.send(RpcEnvelope::local(msg))
                            .await
                            .map_err(|_e| {
                                SgxMessageError::Service("fatal: exe-unit disconnected".to_string())
                            })?
                            .map_err(rpc_to_sgx_error),
                    )
                }
            })
        }
        .then(move |v| {
            let response = match v {
                Err(e) => Response::Error(e),
                Ok(v) => v,
            };
            match enc.encrypt(&response) {
                Ok(bytes) => future::ok(bytes),
                Err(err) => future::err(RpcMessageError::BadRequest(format!(
                    "Encryption error: {:?}",
                    err
                ))),
            }
        })
        .boxed_local()
    }
}

#[cfg(feature = "sgx")]
fn rpc_to_sgx_error(error: RpcMessageError) -> SgxMessageError {
    match error {
        RpcMessageError::Service(m) => SgxMessageError::Service(m),
        RpcMessageError::Activity(m) => SgxMessageError::Activity(m),
        RpcMessageError::BadRequest(m) => SgxMessageError::BadRequest(m),
        RpcMessageError::UsageLimitExceeded(m) => SgxMessageError::UsageLimitExceeded(m),
        RpcMessageError::NotFound(m) => SgxMessageError::NotFound(m),
        RpcMessageError::Forbidden(m) => SgxMessageError::Forbidden(m),
        RpcMessageError::Timeout => SgxMessageError::Timeout,
    }
}
