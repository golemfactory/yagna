use crate::message::*;
use crate::ExeUnitContext;
use actix::prelude::*;

pub mod process;

pub trait Runtime:
    Actor<Context = Context<Self>> + Handler<Shutdown> + Handler<ExecCmd> + Send + Sync
{
    fn with_context(self, ctx: ExeUnitContext) -> Self;
}
