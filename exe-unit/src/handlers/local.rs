use crate::commands::*;
use crate::error::Error;
use crate::runtime::{Runtime, RuntimeThreadExt};
use crate::service::ServiceAddr;
use crate::{report, ExeUnit};
use actix::prelude::*;
use ya_core_model::activity::SetActivityState;
use ya_model::activity::{ActivityState, State};

impl<R: Runtime> Handler<SetState> for ExeUnit<R> {
    type Result = <SetState as Message>::Result;

    fn handle(&mut self, msg: SetState, ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            SetState::State(state) => {
                let state_differs = self.state.state != state;
                self.state.state = state;

                if let StateExt::State(state) = &self.state.state {
                    if !state_differs {
                        return;
                    }

                    let report_url = self.report_url.clone();
                    let activity_id = match &self.ctx.service_id {
                        Some(id) => id.clone(),
                        None => return,
                    };
                    let activity_state = ActivityState {
                        state: state.clone(),
                        reason: None,
                        error_message: None,
                    };

                    let fut = async move {
                        report(
                            &report_url,
                            SetActivityState {
                                activity_id,
                                state: activity_state,
                                timeout: None,
                            },
                        )
                        .await;
                    };

                    ctx.spawn(fut.into_actor(self));
                }
            }
            SetState::BatchResult(batch_id, result) => self.state.push_result(batch_id, result),
            SetState::RunningCommand(command) => self.state.running_command = command,
        }
    }
}

impl<Svc, R> Handler<RegisterService<Svc>> for ExeUnit<R>
where
    R: Runtime,
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>,
{
    type Result = <RegisterService<Svc> as Message>::Result;

    fn handle(&mut self, msg: RegisterService<Svc>, _: &mut Context<Self>) -> Self::Result {
        self.services.push(Box::new(ServiceAddr::new(msg.0)));
    }
}

impl<R: Runtime> Handler<Shutdown> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        let state = self.state.state.clone();
        if state != StateExt::ShuttingDown && state != StateExt::State(State::Terminated) {
            self.state.state = StateExt::ShuttingDown;

            let address = ctx.address();
            let runtime = self.runtime.flatten_addr();
            let mut services = std::mem::replace(&mut self.services, Vec::new());

            let fut = async move {
                if let Some(runtime) = runtime {
                    Self::stop_runtime(runtime, msg.0).await;
                }

                services.iter_mut().for_each(|svc| svc.stop());

                if let Err(e) = address
                    .send(SetState::State(StateExt::State(State::Terminated)))
                    .await
                {
                    log::error!("Error updating state to {:?}: {:?}", State::Terminated, e);
                }

                Arbiter::current().stop();
                Ok(())
            };

            ActorResponse::r#async(fut.into_actor(self))
        } else {
            let fut = async move {
                log::warn!("Shutdown already triggered");
                Ok(())
            };

            ActorResponse::r#async(fut.into_actor(self))
        }
    }
}
