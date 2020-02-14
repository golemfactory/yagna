use crate::commands::Signal;
use crate::runtime::Runtime;
use crate::ExeUnit;
use actix::prelude::*;
use ya_core_model::activity::*;
use ya_service_bus::RpcEnvelope;

impl<R: Runtime> Handler<RpcEnvelope<Exec>> for ExeUnit<R> {
    type Result = <RpcEnvelope<Exec> as Message>::Result;

    fn handle(&mut self, msg: RpcEnvelope<Exec>, ctx: &mut Self::Context) -> Self::Result {
        unimplemented!()
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetActivityState>> for ExeUnit<R> {
    type Result = <RpcEnvelope<GetActivityState> as Message>::Result;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetActivityState>,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        unimplemented!()
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetActivityUsage>> for ExeUnit<R> {
    type Result = <RpcEnvelope<GetActivityUsage> as Message>::Result;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetActivityUsage>,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        unimplemented!()
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetRunningCommand>> for ExeUnit<R> {
    type Result = <RpcEnvelope<GetRunningCommand> as Message>::Result;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetRunningCommand>,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        unimplemented!()
    }
}

impl<R: Runtime> Handler<RpcEnvelope<GetExecBatchResults>> for ExeUnit<R> {
    type Result = <RpcEnvelope<GetExecBatchResults> as Message>::Result;

    fn handle(
        &mut self,
        msg: RpcEnvelope<GetExecBatchResults>,
        ctx: &mut Self::Context,
    ) -> Self::Result {
        unimplemented!()
    }
}

//impl<R: Runtime> Handler<GetState> for ExeUnit<R> {
//    type Result = <GetState as Message>::Result;
//
//    fn handle(&mut self, msg: GetState, ctx: &mut Context<Self>) -> Self::Result {
//        self.state.state.clone()
//    }
//}
//
//impl<R: Runtime> Handler<GetRunningCommand> for ExeUnit<R> {
//    type Result = <GetRunningCommand as Message>::Result;
//
//    fn handle(&mut self, msg: GetRunningCommand, ctx: &mut Context<Self>) -> Self::Result {
//        self.state.running_command.cloned()
//    }
//}
//
//impl<R: Runtime> Handler<GetBatchResults> for ExeUnit<R> {
//    type Result = <GetBatchResults as Message>::Result;
//
//    fn handle(&mut self, msg: GetBatchResults, ctx: &mut Context<Self>) -> Self::Result {
//        self.state.get_results(&msg.batch_id)
//    }
//}
