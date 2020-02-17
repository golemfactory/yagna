use crate::commands::*;
use crate::error::{Error, LocalServiceError};
use crate::runtime::{Runtime, RuntimeThreadExt};
use crate::service::{Service, ServiceAddr};
use crate::ExeUnit;
use actix::prelude::*;
use ya_model::activity::State;

impl<S: Service, R: Runtime> Handler<RegisterService<S>> for ExeUnit<R> {
    type Result = <RegisterService<S> as Message>::Result;

    fn handle(&mut self, msg: RegisterService<S>, _: &mut Context<Self>) -> Self::Result {
        self.services.push(Box::new(ServiceAddr::new(msg.0)));
    }
}

impl<R: Runtime> Handler<SetState> for ExeUnit<R> {
    type Result = <SetState as Message>::Result;

    fn handle(&mut self, msg: SetState, _: &mut Context<Self>) -> Self::Result {
        match msg {
            SetState::State(state) => self.state.state = state,
            SetState::BatchResult(batch_id, result) => self.state.push_result(batch_id, result),
            SetState::RunningCommand(command) => self.state.running_command = command,
        }
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
