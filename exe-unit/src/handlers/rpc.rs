use crate::commands::{MetricsRequest, StateExt};
use crate::error::Error;
use crate::runtime::Runtime;
use crate::ExeUnit;
use actix::prelude::*;
use ya_core_model::activity::*;
use ya_model::activity::{ActivityState, ActivityUsage, State};
use ya_service_bus::RpcEnvelope;

impl<R: Runtime> Handler<RpcEnvelope<Exec>> for ExeUnit<R> {
    type Result = <RpcEnvelope<Exec> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<Exec>, _ctx: &mut Self::Context) -> Self::Result {
        self.match_service_id(&msg.activity_id)?;

        Ok(msg.batch_id.clone())
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetActivityState>> for ExeUnit<R> {
    type Result = <RpcEnvelope<GetActivityState> as Message>::Result;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetActivityState>,
        _: &mut Self::Context,
    ) -> Self::Result {
        self.match_service_id(&msg.activity_id)?;

        let state = match &self.state.inner {
            StateExt::State(state) => state.clone(),
            StateExt::Transitioning {
                from: _,
                to: State::Terminated,
            } => State::Terminated,
            StateExt::Transitioning { from, .. } => from.clone(),
        };

        Ok(ActivityState {
            state,
            reason: None,
            error_message: None,
        })
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetActivityUsage>> for ExeUnit<R> {
    type Result = ActorResponse<Self, ActivityUsage, RpcMessageError>;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetActivityUsage>,
        _: &mut Self::Context,
    ) -> Self::Result {
        if let Err(e) = self.match_service_id(&msg.activity_id) {
            return ActorResponse::r#async(futures::future::err(e.into()).into_actor(self));
        }

        let metrics = self.metrics.clone();
        let fut = async move {
            let resp = match metrics.send(MetricsRequest).await {
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
        self.match_service_id(&msg.activity_id)?;

        match &self.state.running_command {
            Some(command) => Ok(command.clone()),
            None => Err(RpcMessageError::NotFound),
        }
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetExecBatchResults>> for ExeUnit<R> {
    type Result = <RpcEnvelope<GetExecBatchResults> as Message>::Result;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetExecBatchResults>,
        _: &mut Self::Context,
    ) -> Self::Result {
        self.match_service_id(&msg.activity_id)?;

        let msg = msg.into_inner();
        match self.state.batch_results.get(&msg.batch_id) {
            Some(batch) => Ok(batch.clone()),
            None => Ok(Vec::new()),
        }
    }
}
