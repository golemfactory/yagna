use actix_rt::Arbiter;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Duration, Utc};
use futures::future::{AbortHandle, Abortable, LocalBoxFuture};
use futures::FutureExt;
use tokio::sync::RwLock;
use ya_client_model::NodeId;

use ya_core_model::driver;
use ya_core_model::driver::{AccountMode, BatchMode};
use ya_core_model::payment::local as pay;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;

use crate::dao::BatchDao;
use crate::error::processor::SchedulePaymentError;
use crate::models::batch::{DbBatchOrder, DbBatchOrderItem};
use crate::processor::AccountDetails;

lazy_static::lazy_static! {
    /// Maximum time required to process the payment
    static ref DEFAULT_PAYMENT_PROCESSING_TIME: Duration = Duration::seconds(60);
}

#[derive(Clone)]
pub struct BatchScheduler {
    db: DbExecutor,
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    handle: Option<AbortHandle>,
    scheduled_at: DateTime<Utc>,
    since: DateTime<Utc>,
    history: HashSet<(NodeId, String, String)>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            handle: None,
            scheduled_at: Utc::now() + chrono::Duration::days(3650),
            since: DateTime::<Utc>::from(SystemTime::UNIX_EPOCH),
            history: Default::default(),
        }
    }
}

impl BatchScheduler {
    pub fn new(db: DbExecutor) -> Self {
        let inner = Default::default();
        Self { db, inner }
    }

    pub async fn add_payment(
        &self,
        account: AccountDetails,
        msg: pay::SchedulePayment,
    ) -> Result<(), SchedulePaymentError> {
        let now = Utc::now();
        let since;
        {
            let mut inner = self.inner.write().await;
            inner.history.insert((
                msg.payer_id,
                msg.payer_addr.clone(),
                msg.payment_platform.clone(),
            ));
            since = inner.since;
        };

        let mut at = msg.due_date - processing_time_needed(&account);
        at = if at <= now { now } else { at };
        let mut due_date = msg.due_date;
        due_date = if due_date <= now { now } else { due_date };

        let latest = collect_payments(
            self.db.clone(),
            msg.payer_id,
            msg.payer_addr,
            msg.payment_platform,
            since,
            now,
        )
        .await
        .map_err(|e| SchedulePaymentError::Batch(e.to_string()))?;

        let mut inner = self.inner.write().await;
        inner.since = latest;

        if inner.scheduled_at > at || inner.scheduled_at < now {
            let (h, reg) = AbortHandle::new_pair();
            inner.handle.take().map(|h| h.abort());
            inner.handle.replace(h);
            inner.scheduled_at = at;

            log::info!("Batch payment is scheduled at {}", at);

            let send_at = send_payments_at(self.db.clone(), at, Some(due_date));
            Arbiter::spawn(Abortable::new(send_at, reg).then(|r| async move { () }));
        } else {
            log::debug!("Batch payment schedule unchanged: {}", inner.scheduled_at);
        }

        Ok(())
    }

    pub fn shutdown<'a>(&self) -> LocalBoxFuture<'a, ()> {
        let db = self.db.clone();
        let inner = self.inner.clone();
        async move {
            let now = Utc::now();
            let (since, history) = {
                let mut inner = inner.write().await;
                inner.handle.take().map(|h| h.abort());
                (
                    inner.since,
                    std::mem::replace(&mut inner.history, Default::default()),
                )
            };

            for (id, addr, platform) in history {
                if let Err(e) = collect_payments(db.clone(), id, addr, platform, since, now).await {
                    log::warn!("Unable to collect payments: {}", e);
                }
            }

            log::info!("Executing all remaining batch payments");
            send_payments_at(db, Utc::now(), None).await;
        }
        .boxed_local()
    }
}

async fn collect_payments(
    db: DbExecutor,
    payer_id: NodeId,
    payer_addr: String,
    payment_platform: String,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, SchedulePaymentError> {
    log::debug!(
        "Collecting batch for {} ({}) since {}",
        payer_addr,
        payment_platform,
        since
    );

    let now = Utc::now();
    let (_, date) = db
        .as_dao::<BatchDao>()
        .batch(payer_id, payer_addr, payment_platform, since, now)
        .await?;

    Ok(date)
}

async fn send_payments_at(db: DbExecutor, at: DateTime<Utc>, due_date: Option<DateTime<Utc>>) {
    let mut delay = std::time::Duration::from_secs(0);
    let now = Utc::now();

    if at > now {
        if let Ok(d) = (at - now).to_std() {
            delay = d;
        }
    }

    log::debug!("Executing next batch payment in {}s", delay.as_secs());

    tokio::time::delay_for(delay).await;
    if let Err(e) = send_payments(db, due_date).await {
        log::warn!("Unable to send batch payments: {}", e);
    }
}

async fn send_payments(
    db: DbExecutor,
    due_date: Option<DateTime<Utc>>,
) -> Result<(), SchedulePaymentError> {
    let entries = db
        .as_dao::<BatchDao>()
        .get_unsent_batch_orders(due_date)
        .await?;

    if entries.is_empty() {
        log::info!("No batch payments to send");
        return Ok(());
    }

    log::info!("Sending {} batch payment(s)", entries.len());

    for (item, order) in entries {
        let amount = order.total_amount.unwrap_or(0.);
        let payee = item.payee_addr.clone();
        let platform = order.platform.clone();

        if let Err(err) = send_payment(db.clone(), item, order).await {
            log::warn!(
                "Batch payment of {} to {} ({}) failed: {}",
                amount,
                payee,
                platform,
                err
            );
        }
    }

    Ok(())
}

async fn send_payment(
    db: DbExecutor,
    item: DbBatchOrderItem,
    order: DbBatchOrder,
) -> Result<(), SchedulePaymentError> {
    let account = bus::service(pay::BUS_ID)
        .call(pay::GetAccount {
            platform: order.platform.clone(),
            address: order.payer_addr.clone(),
            mode: AccountMode::SEND,
        })
        .await
        .map_err(|_| SchedulePaymentError::Batch("payment service is not available".to_string()))?
        .map_err(|e| SchedulePaymentError::Batch(e.to_string()))?
        .ok_or_else(|| SchedulePaymentError::Batch("payment account not found".to_string()))?;

    let bus_id = driver::driver_bus_id(&account.driver);
    let driver_order_id = bus::service(&bus_id)
        .call(driver::SchedulePayment::new(
            item.amount.0,
            order.payer_addr.clone(),
            item.payee_addr.clone(),
            order.platform.clone(),
            chrono::Utc::now(),
        ))
        .await??;

    db.as_dao::<BatchDao>()
        .batch_order_item_send(order.id, item.payee_addr, driver_order_id)
        .await?;
    Ok(())
}

fn processing_time_needed(account: &AccountDetails) -> chrono::Duration {
    account
        .batch
        .as_ref()
        .map(|b| match b {
            BatchMode::Auto {
                max_processing_time,
            } => Duration::seconds(*max_processing_time as i64),
            _ => *DEFAULT_PAYMENT_PROCESSING_TIME,
        })
        .unwrap_or(*DEFAULT_PAYMENT_PROCESSING_TIME)
}
