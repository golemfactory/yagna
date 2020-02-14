pub mod signal;

use crate::Result;
use actix::{Actor, Addr};
use std::fmt::Debug;

pub trait Service<A: Actor>: Debug {
    fn start(&mut self, actor: Addr<A>) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
}
