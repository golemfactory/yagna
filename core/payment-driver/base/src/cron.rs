/*
    Manage PaymentDriver tasks to be ran on set intervals.
*/

// Extrernal crates
use actix::AsyncContext;
use actix::{
    prelude::{Addr, Context},
    Actor,
};
use futures::lock::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub use async_trait::async_trait;

#[async_trait(?Send)]
pub trait PaymentDriverCron {
    async fn confirm_payments(&self);
    async fn process_payments(&self);
}

pub struct Cron<D: PaymentDriverCron> {
    payment_job_handle: Arc<Mutex<Arc<D>>>,
    confirmation_job_handle: Arc<Mutex<Arc<D>>>,
}

impl<D: PaymentDriverCron + 'static> Cron<D> {
    pub fn new(driver: Arc<D>) -> Addr<Self> {
        log::trace!("Creating Cron for PaymentDriver.");
        let me = Self {
            payment_job_handle: Arc::new(Mutex::new(driver.clone())),
            confirmation_job_handle: Arc::new(Mutex::new(driver.clone())),
        };
        me.start()
    }

    fn start_confirmation_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(5), |act, _ctx| {
            let driver = act.confirmation_job_handle.clone();
            tokio::task::spawn_local(async move {
                match driver.try_lock() {
                    Some(driver) => {
                        log::trace!("Running payment confirmation job...");
                        driver.confirm_payments().await;
                        log::trace!("Confirmation job finished.");
                    }
                    None => {
                        log::trace!("Confirmation job in progress.");
                    }
                }
            });
        });
    }

    fn start_payment_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(10), |act, _ctx| {
            let driver = act.payment_job_handle.clone();
            tokio::task::spawn_local(async move {
                match driver.try_lock() {
                    Some(driver) => {
                        log::trace!("Running payment job...");
                        driver.process_payments().await;
                        log::trace!("Payment job finished.");
                    }
                    None => {
                        log::trace!("Payment job in progress.");
                    }
                }
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
