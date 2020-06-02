use crate::message::*;
use actix::prelude::*;

pub mod process;

pub trait Runtime:
    Actor<Context = Context<Self>>
    + Handler<Shutdown>
    + Handler<ExecCmd>
    + Handler<SetTaskPackagePath>
    + Send
    + Sync
{
}
