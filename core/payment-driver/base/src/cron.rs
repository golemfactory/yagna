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
    async fn confirm_payments(&self);
    async fn process_payments(&self);
}

pub struct Cron<D: PaymentDriverCron> {
    driver: Arc<D>,
}

impl<D: PaymentDriverCron + 'static> Cron<D> {
    pub fn new(driver: Arc<D>) -> Addr<Self> {
        log::trace!("Creating Cron for PaymentDriver.");
        let me = Self { driver };
        me.start()
    }

    fn start_confirmation_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(5), |act, _ctx| {
            log::trace!("Spawning confirmation job.");
            let driver = act.driver.clone();
            Arbiter::spawn(async move {
                driver.confirm_payments().await;
                log::trace!("Confirmation job finished.");
            });
        });
    }

    fn start_payment_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(10), |act, _ctx| {
            log::trace!("Spawning payment job.");
            let driver = act.driver.clone();
            Arbiter::spawn(async move {
                driver.process_payments().await;
                log::trace!("Payment job finished.");
            });
        });
    }
}

impl<D: PaymentDriverCron + 'static> Actor for Cron<D> {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.start_confirmation_job(ctx);
        self.start_payment_job(ctx);
    }
}
