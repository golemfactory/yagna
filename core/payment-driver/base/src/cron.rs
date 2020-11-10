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

pub struct Cron {
    driver: Arc<dyn PaymentDriverCron>,
}

impl Cron {
    pub fn new(driver: Arc<dyn PaymentDriverCron>) -> Addr<Self> {
        log::trace!("Creating Cron for PaymentDriver.");
        let me = Self { driver };
        me.start()
    }

    fn start_confirmation_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(10), |act, _ctx| {
            log::trace!("Spawning confirmation job.");
            let driver = act.driver.clone();
            Arbiter::spawn(async move {
                driver.confirm_payments().await;
            });
        });
    }

    fn start_payment_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(30), |act, _ctx| {
            log::trace!("Spawning payment job.");
            let driver = act.driver.clone();
            Arbiter::spawn(async move {
                driver.process_payments().await;
            });
        });
    }
}

impl Actor for Cron {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.start_confirmation_job(ctx);
        self.start_payment_job(ctx);
    }
}
