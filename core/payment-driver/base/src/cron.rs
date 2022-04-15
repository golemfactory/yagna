/*
    Manage PaymentDriver tasks to be ran on set intervals.
*/

// Extrernal crates
use actix::Arbiter;
use actix::{
    prelude::{Addr, Context},
    Actor,
};
use std::sync::Arc;

use async_trait::async_trait;

#[async_trait(?Send)]
pub trait PaymentDriverCron {
    async fn start_confirmation_job(self: Arc<Self>);
}

pub struct Cron<D: PaymentDriverCron + 'static> {
    driver: Arc<D>,
}

impl<D: PaymentDriverCron + 'static> Cron<D> {
    pub fn new(driver: Arc<D>) -> Addr<Self> {
        log::trace!("Creating Cron for PaymentDriver.");
        let me = Self { driver };
        me.start()
    }

    fn start_confirmation_job(&mut self, _ctx: &mut Context<Self>) {
        let driver = self.driver.clone();
        Arbiter::spawn(async move { driver.start_confirmation_job().await });
    }
}

impl<D: PaymentDriverCron + 'static> Actor for Cron<D> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.start_confirmation_job(ctx);
    }
}
