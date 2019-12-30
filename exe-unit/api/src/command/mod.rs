pub mod from_json;
pub mod many;
pub mod one;

use actix::prelude::*;

pub struct Dispatcher<Ctx>
where
    Ctx: Actor,
{
    pub(crate) worker: Addr<Ctx>,
}

impl<Ctx> Dispatcher<Ctx>
where
    Ctx: Actor,
{
    pub fn new(worker: Addr<Ctx>) -> Self {
        Self { worker }
    }
}

impl<Ctx> Actor for Dispatcher<Ctx>
where
    Ctx: Actor,
{
    type Context = Context<Self>;
}
