use crate::error::Error;
use crate::message::*;
use crate::runtime::Runtime;
use crate::service::ServiceAddr;
use crate::state::State;
use crate::{report, ExeUnit};
use actix::prelude::*;
use ya_core_model::activity::local::SetState as SetActivityState;
use ya_model::activity::ActivityState;

impl<R: Runtime> Handler<GetState> for ExeUnit<R> {
    type Result = <GetState as Message>::Result;

    fn handle(&mut self, _: GetState, _: &mut Context<Self>) -> Self::Result {
        GetStateResult(self.state.inner.clone())
    }
}

impl<R: Runtime> Handler<SetState> for ExeUnit<R> {
    type Result = <SetState as Message>::Result;

    fn handle(&mut self, msg: SetState, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(state) = &msg.state {
            if &self.state.inner != state {
                log::debug!("Entering state: {:?}", state);
                self.state.inner = state.clone();

                if let Some(id) = &self.ctx.activity_id {
                    ctx.spawn(
                        report(
                            self.ctx.report_url.clone().unwrap(),
                            SetActivityState {
                                activity_id: id.clone(),
                                state: ActivityState::from(state),
                                timeout: None,
                            },
                        )
                        .into_actor(self),
                    );
                }
            }
        }

        if let Some(running_command) = &msg.running_command {
            self.state.running_command = running_command.clone();
        }

        if let Some(batch_result) = &msg.batch_result {
            self.state
                .push_batch_result(batch_result.0.to_owned(), batch_result.1.to_owned());
        }
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

impl<R: Runtime> Handler<Shutdown> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        if !self.state.inner.alive() {
            return ActorResponse::r#async(async { Ok(()) }.into_actor(self));
        }

        let address = ctx.address();
        let runtime = self.runtime.clone();
        let services = std::mem::replace(&mut self.services, Vec::new());
        let state = self.state.inner.to_pending(State::Terminated);

        let fut = async move {
            log::info!("Shutting down ...");
            let _ = address.send(SetState::from(state)).await;

            Self::stop_runtime(runtime, msg.0).await;
            for mut service in services.into_iter() {
                service.stop().await;
            }

            let _ = address.send(SetState::from(State::Terminated)).await;

            System::current().stop();

            log::info!("Shutdown process complete");
            Ok(())
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}
