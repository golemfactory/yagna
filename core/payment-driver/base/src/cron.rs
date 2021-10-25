/*
    Manage PaymentDriver tasks to be ran on set intervals.
*/

// Extrernal crates
use actix::Arbiter;
use actix::AsyncContext;
use actix::{
    prelude::{Addr, Context},
    Actor,
};
use std::sync::Arc;
use std::time::Duration;

pub use async_trait::async_trait;

#[async_trait(?Send)]
pub trait PaymentDriverCron {
    fn sendout_interval(&self) -> Duration;
    fn confirmation_interval(&self) -> Duration;
    async fn send_out_payments(&self);
    async fn confirm_payments(&self);
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

    fn start_confirmation_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(self.driver.confirmation_interval(), |act, _ctx| {
            let driver = act.driver.clone();
            Arbiter::spawn(async move
                {
                    driver.confirm_payments().await;
                    driver.send_out_payments().await;
                });
        });
    }
/*
    fn start_sendout_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(self.driver.sendout_interval(), |act, _ctx| {
            let driver = act.driver.clone();
            Arbiter::spawn(async move {  });
        });
    }*/
}

impl<D: PaymentDriverCron + 'static> Actor for Cron<D> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.start_confirmation_job(ctx);
        //self.start_sendout_job(ctx);
    }
}
