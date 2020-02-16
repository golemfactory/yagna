use crate::commands::*;
use crate::runtime::{Runtime, RuntimeThreadExt};
use crate::ExeUnit;
use actix::prelude::*;
use std::collections::HashMap;
use std::time::Duration;
use ya_model::activity::State;

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

impl<R: Runtime> Handler<Signal> for ExeUnit<R> {
    type Result = <Signal as Message>::Result;

    fn handle(&mut self, msg: Signal, ctx: &mut Context<Self>) -> Self::Result {
        match msg.0 {
            signal_hook::SIGABRT | signal_hook::SIGINT | signal_hook::SIGTERM => {
                ctx.address().do_send(Shutdown::new())
            }
            #[cfg(not(windows))]
            signal_hook::SIGQUIT => ctx.address().do_send(Shutdown::new()),
            #[cfg(not(windows))]
            signal_hook::SIGHUP => {}
            _ => {
                log::warn!("Unsupported signal: {}", msg.0);
            }
        }
    }
}

impl<R: Runtime> Handler<Shutdown> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), LocalError>;

    fn handle(&mut self, _: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        let s = &self.state.state;
        if s != &StateExt::ShuttingDown && s != &StateExt::State(State::Terminated) {
            let address = ctx.address();
            let runtime = self.runtime.flatten_addr();
            let mut services = std::mem::replace(&mut self.services, HashMap::new());

            let fut = async move {
                set_state(&address, StateExt::ShuttingDown).await;

                if let Some(ref r) = runtime {
                    if let Err(e) = r
                        .send(Shutdown::new())
                        .timeout(Duration::from_secs(5u64))
                        .await
                    {
                        log::warn!("Unable to stop the runtime: {:?}", e);
                    }
                }

                services.values_mut().for_each(|svc| svc.stop());
                set_state(&address, StateExt::State(State::Terminated)).await;
                Arbiter::current().stop();
                Ok(())
            };

            ActorResponse::r#async(fut.into_actor(self))
        } else {
            let fut = async move { Err(LocalError::InvalidStateError) };
            ActorResponse::r#async(fut.into_actor(self))
        }
    }
}

#[inline]
async fn set_state<R: Runtime>(address: &Addr<ExeUnit<R>>, state: StateExt) {
    if let Err(e) = address.send(SetState::State(state)).await {
        log::error!("Error updating state to {:?}: {:?}", State::Terminated, e);
    }
}
