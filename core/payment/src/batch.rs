use crate::dao::BatchDao;
use crate::models::batch::DbBatchOrderItemFullInfo;
use crate::timeout_lock::MutexTimeoutExt;
use bigdecimal::{BigDecimal, Zero};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ya_client_model::payment::allocation::Deposit;
use ya_client_model::NodeId;
use ya_core_model::driver::{driver_bus_id, ScheduleDriverPayment};
use ya_core_model::payment::local::ProcessPaymentsError;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::timeout::IntoTimeoutFuture;
use ya_service_bus::typed as bus;

fn get_order_lock(order_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    lazy_static! {
        static ref ORDER_LOCKS: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
    }

    ORDER_LOCKS
        .lock()
        .unwrap()
        .entry(order_id.to_string())
        .or_default()
        .clone()
}

async fn send_deposit_payments(
    db: Arc<tokio::sync::Mutex<DbExecutor>>,
    owner: NodeId,
    order_id: &str,
    deposit_items: Vec<DbBatchOrderItemFullInfo>,
) -> anyhow::Result<()> {
    //Send payments from deposits
    let bus_id = driver_bus_id("erc20");

    for item in deposit_items {
        let deposit = item.deposit.ok_or(anyhow::anyhow!(
            "deposit is missing in send_deposit_payments, probably logic error in application code"
        ))?;

        let deposit = serde_json::from_str::<Deposit>(&deposit)
            .map_err(|err| anyhow::anyhow!("failed to parse deposit: {:?}", err))?;

        let payment_order_id = bus::service(&bus_id)
            .call(ScheduleDriverPayment::new(
                item.amount.0,
                item.payer_addr.clone(),
                item.payee_addr.clone(),
                item.platform.clone(),
                Some(deposit),
                chrono::Utc::now(),
            ))
            .await??;

        {
            let db_executor = db
                .timeout_lock(crate::processor::DB_LOCK_TIMEOUT)
                .await
                .map_err(|err| {
                    ProcessPaymentsError::ProcessPaymentsError(format!(
                        "Db timeout lock when sending payments {err}"
                    ))
                })?;
            db_executor
                .as_dao::<BatchDao>()
                .batch_order_item_send(
                    order_id.to_string(),
                    owner,
                    item.payee_addr,
                    item.allocation_id,
                    payment_order_id,
                )
                .await?;
        }
    }
    Ok(())
}

pub async fn send_batch_payments(
    db: Arc<tokio::sync::Mutex<DbExecutor>>,
    owner: NodeId,
    order_id: &str,
) -> anyhow::Result<()> {
    // Whole operation send payments should be in transaction, but it's not possible because we have to send messages via GSB.
    // So we have to lock order_id to prevent sending payments for the same order in parallel.
    // It's not perfect but at least it gives some hopes that data stays consistent.

    {
        //_lock has to stay in scope to keep the lock
        let order_lock = get_order_lock(order_id);
        let _lock = order_lock
            .lock()
            .timeout(Some(Duration::from_secs(60)))
            .await
            .map_err(|err| {
                anyhow::anyhow!(
                    "failed to acquire lock for order_id: {:?}, err: {:?}",
                    order_id,
                    err
                )
            })?;

        let items = {
            let db_executor = db
                .timeout_lock(crate::processor::DB_LOCK_TIMEOUT)
                .await
                .map_err(|err| {
                    ProcessPaymentsError::ProcessPaymentsError(format!(
                        "Db timeout lock when sending payments {err}"
                    ))
                })?;
            db_executor
                .as_dao::<BatchDao>()
                .get_unsent_batch_items(owner, order_id.to_string())
                .await?
        };

        //group items without deposit
        let mut map: HashMap<BatchOrderItemKey, Vec<DbBatchOrderItemFullInfo>> = HashMap::new();
        //the rest of items go to deposit_items
        let mut deposit_items = Vec::new();

        for item in items {
            if item.deposit.is_none() {
                map.entry(BatchOrderItemKey {
                    order_id: item.order_id.clone(),
                    platform: item.platform.clone(),
                    owner_id: item.owner_id.clone(),
                    payer_addr: item.payer_addr.clone(),
                    payee_addr: item.payee_addr.clone(),
                })
                .or_default()
                .push(item);
            } else {
                deposit_items.push(item);
            }
        }
        send_deposit_payments(db.clone(), owner, order_id, deposit_items).await?;
        let bus_id = driver_bus_id("erc20");

        for (key, items) in map {
            //sum item amount
            let mut payment_group_amount = BigDecimal::zero();
            for item in &items {
                payment_group_amount += item.amount.0.clone();
            }

            let payment_order_id = bus::service(&bus_id)
                .call(ScheduleDriverPayment::new(
                    payment_group_amount,
                    key.payer_addr.clone(),
                    key.payee_addr.clone(),
                    key.platform.clone(),
                    None,
                    chrono::Utc::now(),
                ))
                .await??;
            for item in items {
                let db_executor = db
                    .timeout_lock(crate::processor::DB_LOCK_TIMEOUT)
                    .await
                    .map_err(|err| {
                        ProcessPaymentsError::ProcessPaymentsError(format!(
                            "Db timeout lock when sending payments {err}"
                        ))
                    })?;
                db_executor
                    .as_dao::<BatchDao>()
                    .batch_order_item_send(
                        order_id.to_string(),
                        owner,
                        key.payee_addr.clone(),
                        item.allocation_id,
                        payment_order_id.clone(),
                    )
                    .await?;
            }
        }

        //End of _lock
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct BatchOrderItemKey {
    pub order_id: String,
    pub platform: String,
    pub owner_id: String,
    pub payer_addr: String,
    pub payee_addr: String,
}
