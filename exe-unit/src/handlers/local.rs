use crate::error::Error;
use crate::message::*;
use crate::runtime::Runtime;
use crate::service::ServiceAddr;
use crate::state::State;
use crate::{report, ExeUnit};
use actix::prelude::*;
use futures::FutureExt;
use ya_client_model::activity::{ActivityState, RuntimeEvent, RuntimeEventKind};
use ya_core_model::activity::local::SetState as SetActivityState;

impl<R: Runtime> StreamHandler<RuntimeEvent> for ExeUnit<R> {
    fn handle(&mut self, update: RuntimeEvent, _: &mut Context<Self>) {
        let batch = match self.state.batches.get_mut(&update.batch_id) {
            Some(batch) => batch,
            _ => return log::warn!("Batch event error: unknown batch {}", update.batch_id),
        };

        self.state.last_batch = Some(update.batch_id.clone());

        if let Err(err) = match &update.kind {
            RuntimeEventKind::Started { command: _ } => batch.started(update.index),
            RuntimeEventKind::StdOut(out) => batch.push_stdout(update.index, out.clone()),
            RuntimeEventKind::StdErr(out) => batch.push_stderr(update.index, out.clone()),
            RuntimeEventKind::Finished {
                return_code,
                message,
            } => batch.finished(update.index, *return_code, message.clone()),
        } {
            log::error!("Batch {} event error: {}", update.batch_id, err);
        }

        if batch.stream.initialized() {
            if let Err(err) = batch.stream.sender().send(update) {
                log::warn!(
                    "Batch {} event stream interrupted: the receiver is gone",
                    err.0.batch_id
                );
            }
        }
    }
}

impl<R: Runtime> Handler<GetState> for ExeUnit<R> {
    type Result = <GetState as Message>::Result;

    fn handle(&mut self, _: GetState, _: &mut Context<Self>) -> Self::Result {
        GetStateResponse(self.state.inner)
    }
}

impl<R: Runtime> Handler<SetState> for ExeUnit<R> {
    type Result = <SetState as Message>::Result;

    fn handle(&mut self, update: SetState, ctx: &mut Context<Self>) -> Self::Result {
        if self.state.inner == update.state {
            return;
        }

        log::debug!("Entering state: {:?}", update.state);
        log::debug!("Report: {}", self.state.report());
        self.state.inner = update.state.clone();

        if self.ctx.activity_id.is_none() || self.ctx.report_url.is_none() {
            return;
        }
        let fut = report(
            self.ctx.report_url.clone().unwrap(),
            SetActivityState {
                activity_id: self.ctx.activity_id.clone().unwrap(),
                state: ActivityState {
                    state: update.state,
                    reason: update.reason,
                    error_message: None,
                },
                timeout: None,
            },
        );
        ctx.spawn(fut.into_actor(self));
    }
}

impl<R: Runtime> Handler<GetStdOut> for ExeUnit<R> {
    type Result = <GetStdOut as Message>::Result;

    fn handle(&mut self, msg: GetStdOut, _: &mut Context<Self>) -> Self::Result {
        self.state
            .batches
            .get(&msg.batch_id)
            .map(|b| b.results.get(msg.idx).map(|r| r.stdout.clone()).flatten())
            .flatten()
    }
}

impl<R: Runtime> Handler<GetBatchResults> for ExeUnit<R> {
    type Result = <GetBatchResults as Message>::Result;

    fn handle(&mut self, msg: GetBatchResults, _: &mut Context<Self>) -> Self::Result {
        let results = match self.state.batches.get(&msg.0) {
            Some(batch) => batch.results(),
            _ => {
                log::warn!("Batch results error: unknown batch {}", msg.0);
                Vec::new()
            }
        };
        GetBatchResultsResponse(results)
    }
}

impl<Svc, R> Handler<Register<Svc>> for ExeUnit<R>
where
    R: Runtime,
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>,
{
    type Result = <Register<Svc> as Message>::Result;

    fn handle(&mut self, msg: Register<Svc>, _: &mut Context<Self>) -> Self::Result {
        self.services.push(Box::new(ServiceAddr::new(msg.0)));
    }
}

impl<R: Runtime> Handler<Stop> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Stop, _: &mut Context<Self>) -> Self::Result {
        self.state.batches.iter_mut().for_each(|(id, batch)| {
            if msg.exclude_batches.contains(id) {
                return;
            }
            if let Some(tx) = batch.control.take() {
                let _ = tx.send(());
            }
        });

        let fut = Self::stop_runtime(self.runtime.clone(), ShutdownReason::Interrupted(0))
            .map(|_| Ok(()));
        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl<R: Runtime> Handler<Shutdown> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        if !self.state.inner.alive() {
            return ActorResponse::r#async(async { Ok(()) }.into_actor(self));
        }

        let address = ctx.address();
        let services = std::mem::replace(&mut self.services, Vec::new());
        let state = self.state.inner.to_pending(State::Terminated);
        let reason = format!("{}: {}", msg.0, self.state.report());

        let fut = async move {
            log::info!("Shutting down: {}", reason);
            let _ = address.send(SetState::from(state)).await;
            let _ = address.send(Stop::default()).await;

            for mut service in services {
                service.stop().await;
            }

            let set_state = SetState::new(State::Terminated.into(), reason);
            let _ = address.send(set_state).await;

            System::current().stop();

            log::info!("Shutdown process complete");
            Ok(())
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}
