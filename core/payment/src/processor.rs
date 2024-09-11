use crate::api::allocations::{forced_release_allocation, release_allocation_after};
use crate::cycle::BatchCycleTaskManager;
use crate::dao::{
    ActivityDao, AgreementDao, AllocationDao, BatchCycleDao, BatchDao, BatchItemFilter, PaymentDao,
    SyncNotifsDao,
};
use crate::error::processor::{
    AccountNotRegistered, GetStatusError, NotifyPaymentError, ValidateAllocationError,
    VerifyPaymentError,
};
use crate::error::DbResult;
use crate::models::cycle::DbPayBatchCycle;
use crate::payment_sync::SYNC_NOTIFS_NOTIFY;
use crate::timeout_lock::{MutexTimeoutExt, RwLockTimeoutExt};

use actix_web::web::Data;
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use diesel::sql_types::Text;
use diesel::RunQueryDsl;
use futures::{FutureExt, TryFutureExt};
use metrics::counter;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::ops::Sub;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

use ya_client_model::payment::allocation::Deposit;
use ya_client_model::payment::{
    Account, ActivityPayment, AgreementPayment, DriverDetails, Network, Payment,
};
use ya_core_model::driver::{
    self, driver_bus_id, AccountMode, DriverReleaseDeposit, GetAccountBalanceResult,
    GetRpcEndpointsResult, PaymentConfirmation, PaymentDetails, ScheduleDriverPayment, ShutDown,
    ValidateAllocation, ValidateAllocationResult,
};
use ya_core_model::payment::local::{
    CollectPayments, GenericError, GetAccountsError, GetDriversError, NotifyPayment,
    ProcessBatchCycleError, ProcessBatchCycleInfo, ProcessBatchCycleResponse, ProcessBatchCycleSet,
    ProcessPaymentsError, ProcessPaymentsNow, ProcessPaymentsNowResponse, RegisterAccount,
    RegisterAccountError, RegisterDriver, RegisterDriverError, UnregisterAccount,
    UnregisterAccountError, UnregisterDriver, UnregisterDriverError,
};
use ya_core_model::payment::public::{SendPayment, SendSignedPayment, BUS_ID};
use ya_core_model::{identity, NodeId};
use ya_net::RemoteEndpoint;
use ya_persistence::executor::{do_with_transaction, DbExecutor};
use ya_persistence::types::Role;
use ya_service_bus::typed::{service, Endpoint};
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

pub struct ReleaseDeposit {
    pub platform: String,
    pub from: String,
    pub deposit_contract: String,
    pub deposit_id: String,
}

fn driver_endpoint(driver: &str) -> Endpoint {
    bus::service(driver_bus_id(driver))
}

#[derive(Clone, Debug)]
struct AccountDetails {
    pub driver: String,
    pub network: String,
    pub token: String,
    pub mode: AccountMode,
}

#[derive(Clone, Default)]
struct DriverRegistry {
    accounts: HashMap<(String, String), AccountDetails>,
    // (platform, address) -> details
    drivers: HashMap<String, DriverDetails>,
    // driver_name -> details
    platforms: HashMap<String, HashMap<String, bool>>, // platform -> (driver_name -> recv_init_required)
}

impl DriverRegistry {
    pub fn register_driver(&mut self, msg: RegisterDriver) -> Result<(), RegisterDriverError> {
        let RegisterDriver {
            driver_name,
            details,
        } = msg;
        log::trace!(
            "register_driver: driver_name={} details={:?}",
            driver_name,
            details
        );

        if !details.networks.contains_key(&details.default_network) {
            return Err(RegisterDriverError::InvalidDefaultNetwork(
                details.default_network,
            ));
        }
        for (network_name, network) in details.networks.iter() {
            if !network.tokens.contains_key(&network.default_token) {
                return Err(RegisterDriverError::InvalidDefaultToken(
                    network.default_token.clone(),
                    network_name.to_string(),
                ));
            }
            for (token, platform) in network.tokens.iter() {
                self.platforms
                    .entry(platform.clone())
                    .or_default()
                    .insert(driver_name.clone(), details.recv_init_required);
            }
        }
        self.drivers.insert(driver_name, details);
        Ok(())
    }

    pub fn unregister_driver(&mut self, msg: UnregisterDriver) {
        let driver_name = msg.0;
        let details = self.drivers.remove(&driver_name);
        if let Some(details) = details {
            for (network_name, network) in details.networks.iter() {
                for (token, platform) in network.tokens.iter() {
                    self.platforms
                        .entry(platform.clone())
                        .or_default()
                        .remove(&driver_name);
                }
            }
        }
        self.accounts
            .retain(|_, details| details.driver != driver_name);
    }

    pub fn register_account(&mut self, msg: RegisterAccount) -> Result<(), RegisterAccountError> {
        log::info!(
            "register_account: driver={} network={} token={} address={} mode={:?}",
            msg.driver,
            msg.network,
            msg.token,
            msg.address,
            msg.mode
        );
        let driver_details = match self.drivers.get(&msg.driver) {
            None => return Err(RegisterAccountError::DriverNotRegistered(msg.driver)),
            Some(details) => details,
        };
        let network = match driver_details.networks.get(&msg.network) {
            None => {
                return Err(RegisterAccountError::UnsupportedNetwork(
                    msg.network,
                    msg.driver,
                ));
            }
            Some(network) => network,
        };
        let platform = match network.tokens.get(&msg.token) {
            None => {
                return Err(RegisterAccountError::UnsupportedToken(
                    msg.token,
                    msg.network,
                    msg.driver,
                ));
            }
            Some(platform) => platform.clone(),
        };

        match self.accounts.entry((platform, msg.address.clone())) {
            Entry::Occupied(mut entry) => {
                let details = entry.get_mut();
                if details.driver != msg.driver {
                    return Err(RegisterAccountError::AlreadyRegistered(
                        msg.address,
                        details.driver.to_string(),
                    ));
                }
                details.mode |= msg.mode;
            }
            Entry::Vacant(entry) => {
                entry.insert(AccountDetails {
                    driver: msg.driver,
                    network: msg.network,
                    token: msg.token,
                    mode: msg.mode,
                });
            }
        };
        Ok(())
    }

    pub fn unregister_account(&mut self, msg: UnregisterAccount) {
        self.accounts.remove(&(msg.platform, msg.address));
    }

    pub fn get_accounts(&self) -> Vec<Account> {
        self.accounts
            .iter()
            .map(|((platform, address), details)| Account {
                platform: platform.clone(),
                address: address.clone(),
                driver: details.driver.clone(),
                network: details.network.clone(),
                token: details.token.clone(),
                send: details.mode.contains(AccountMode::SEND),
                receive: details.mode.contains(AccountMode::RECV),
            })
            .collect()
    }

    pub fn get_driver(&self, driver: &str) -> Result<&DriverDetails, RegisterAccountError> {
        match self.drivers.get(driver) {
            None => Err(RegisterAccountError::DriverNotRegistered(driver.into())),
            Some(details) => Ok(details),
        }
    }

    pub fn get_drivers(&self) -> HashMap<String, DriverDetails> {
        self.drivers.clone()
    }

    pub fn get_network(
        &self,
        driver: String,
        network: Option<String>,
    ) -> Result<(String, Network), RegisterAccountError> {
        let driver_details = self.get_driver(&driver)?;
        // If network is not specified, use default network
        let network_name = network.unwrap_or_else(|| driver_details.default_network.to_owned());
        match driver_details.networks.get(&network_name) {
            None => Err(RegisterAccountError::UnsupportedNetwork(
                network_name,
                driver,
            )),
            Some(network_details) => Ok((network_name, network_details.clone())),
        }
    }

    pub fn get_platform(
        &self,
        driver: String,
        network: Option<String>,
        token: Option<String>,
    ) -> Result<String, RegisterAccountError> {
        let (network_name, network_details) = self.get_network(driver.clone(), network)?;
        let token = token.unwrap_or_else(|| network_details.default_token.to_owned());
        match network_details.tokens.get(&token) {
            None => Err(RegisterAccountError::UnsupportedToken(
                token,
                network_name,
                driver,
            )),
            Some(platform) => Ok(platform.into()),
        }
    }

    pub fn driver(
        &self,
        platform: &str,
        address: &str,
        mode: AccountMode,
    ) -> Result<String, AccountNotRegistered> {
        if let Some(details) = self
            .accounts
            .get(&(platform.to_owned(), address.to_owned()))
        {
            if details.mode.contains(mode) {
                return Ok(details.driver.clone());
            }
        }

        // If it's recv-only mode or no-mode (i.e. checking status) we can use any driver that
        // supports the given platform and doesn't require init for receiving.
        if !mode.contains(AccountMode::SEND) {
            if let Some(drivers) = self.platforms.get(platform) {
                for (driver, recv_init_required) in drivers.iter() {
                    if !*recv_init_required {
                        return Ok(driver.clone());
                    }
                }
            }
        }

        Err(AccountNotRegistered::new(platform, address, mode))
    }

    pub fn iter_drivers(&self) -> impl Iterator<Item = &String> {
        self.drivers.keys()
    }
}

const DB_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const SCHEDULE_PAYMENT_LOCK_TIMEOUT: Duration = Duration::from_secs(60);
const REGISTRY_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

pub struct PaymentProcessor {
    batch_cycle_tasks: Arc<std::sync::Mutex<BatchCycleTaskManager>>,
    db_executor: Arc<Mutex<DbExecutor>>,
    registry: RwLock<DriverRegistry>,
    in_shutdown: AtomicBool,
    schedule_payment_guard: Arc<Mutex<()>>,
}

#[derive(Debug, PartialEq, Error)]
enum PaymentSendToGsbError {
    #[error("payment Send to Gsb failed")]
    Failed,
    #[error("payment Send to Gsb is not supported")]
    NotSupported,
    #[error("payment Send to Gsb has been rejected")]
    Rejected,
}
pub async fn list_unlocked_identities() -> Result<Vec<NodeId>, ya_core_model::driver::GenericError>
{
    log::debug!("list_unlocked_identities");
    let message = identity::List {};
    let result = service(identity::BUS_ID)
        .send(message)
        .await
        .map_err(ya_core_model::driver::GenericError::new)?
        .map_err(ya_core_model::driver::GenericError::new)?;
    let unlocked_list = result
        .iter()
        .filter(|n| !n.is_locked)
        .map(|n| n.node_id)
        .collect();
    log::debug!(
        "list_unlocked_identities completed. result={:?}",
        unlocked_list
    );
    Ok(unlocked_list)
}

impl PaymentProcessor {
    pub fn new(db_executor: DbExecutor) -> Self {
        Self {
            db_executor: Arc::new(Mutex::new(db_executor)),
            registry: Default::default(),
            in_shutdown: AtomicBool::new(false),
            schedule_payment_guard: Arc::new(Mutex::new(())),
            batch_cycle_tasks: Arc::new(std::sync::Mutex::new(BatchCycleTaskManager::new())),
        }
    }

    pub async fn register_driver(&self, msg: RegisterDriver) -> Result<(), RegisterDriverError> {
        {
            let unlocked_identities = list_unlocked_identities().await.map_err(|err| {
                RegisterDriverError::Other(format!("Error getting unlocked identities: {err}"))
            })?;
            for identity in unlocked_identities {
                self.batch_cycle_tasks.lock().unwrap().add_owner(identity);
            }
        }
        for network in msg.details.networks.keys() {
            self.batch_cycle_tasks.lock().unwrap().add_platform(
                format!(
                    "{}-{}-{}",
                    msg.driver_name,
                    network,
                    msg.details
                        .networks
                        .get(network)
                        .map(|n| n.default_token.clone())
                        .unwrap_or_default()
                )
                .to_lowercase(),
            );
        }

        self.registry
            .timeout_write(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| RegisterDriverError::InternalTimeout)?
            .register_driver(msg)
    }

    pub async fn unregister_driver(
        &self,
        msg: UnregisterDriver,
    ) -> Result<(), UnregisterDriverError> {
        self.registry
            .timeout_write(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| UnregisterDriverError::InternalTimeout)?
            .unregister_driver(msg);

        Ok(())
    }

    pub async fn register_account(&self, msg: RegisterAccount) -> Result<(), RegisterAccountError> {
        self.registry
            .timeout_write(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| RegisterAccountError::InternalTimeout)?
            .register_account(msg)
    }

    pub async fn unregister_account(
        &self,
        msg: UnregisterAccount,
    ) -> Result<(), UnregisterAccountError> {
        self.registry
            .timeout_write(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| UnregisterAccountError::InternalTimeout)?
            .unregister_account(msg);
        Ok(())
    }

    pub async fn get_accounts(&self) -> Result<Vec<Account>, GetAccountsError> {
        self.registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map(|registry| registry.get_accounts())
            .map_err(|_| GetAccountsError::InternalTimeout)
    }

    pub async fn get_drivers(&self) -> Result<HashMap<String, DriverDetails>, GetDriversError> {
        self.registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map(|registry| registry.get_drivers())
            .map_err(|_| GetDriversError::InternalTimeout)
    }

    pub async fn get_network(
        &self,
        driver: String,
        network: Option<String>,
    ) -> Result<(String, Network), RegisterAccountError> {
        self.registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| RegisterAccountError::InternalTimeout)?
            .get_network(driver, network)
    }

    pub async fn get_platform(
        &self,
        driver: String,
        network: Option<String>,
        token: Option<String>,
    ) -> Result<String, RegisterAccountError> {
        self.registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| RegisterAccountError::InternalTimeout)?
            .get_platform(driver, network, token)
    }

    pub async fn process_post_migration_jobs(&self) -> DbResult<()> {
        let db_executor = self
            .db_executor
            .timeout_lock(DB_LOCK_TIMEOUT)
            .await
            .expect("db lock timeout");

        /*
        -- we have to run this query but by hand because of lack of decimal support:
        UPDATE pay_agreement
        SET total_amount_paid = cast(total_amount_paid + (SELECT sum(total_amount_paid)
            FROM pay_activity s
            WHERE s.owner_id = pay_agreement.owner_id
            AND s.role = pay_agreement.role
            AND s.agreement_id = pay_agreement.id) AS VARCHAR)
        WHERE EXISTS (SELECT 1 FROM pay_activity s2 WHERE s2.owner_id = pay_agreement.owner_id
            AND s2.role = pay_agreement.role
            AND s2.agreement_id = pay_agreement.id);

                 */
        #[derive(QueryableByName, PartialEq, Debug)]
        struct JobRecord {
            #[sql_type = "Text"]
            job: String,
        }

        #[derive(QueryableByName, PartialEq, Debug)]
        struct AgreementActivityRecord {
            #[sql_type = "Text"]
            agreement_id: String,
            #[sql_type = "Text"]
            owner_id: String,
            #[sql_type = "Text"]
            role: String,
            #[sql_type = "Text"]
            total_amount_paid_agreement: String,
            #[sql_type = "Text"]
            total_amount_paid_activity: String,
        }

        do_with_transaction(&db_executor.pool, "run_post_migration", move |conn| {
            const JOB_NAME: &str = "sum_activities_into_agreement";
            let job_records = diesel::sql_query(
                r#"
                SELECT job FROM pay_post_migration WHERE done IS NULL AND job = ?
            "#,
            )
            .bind::<Text, _>(JOB_NAME)
            .load::<JobRecord>(conn)?;
            let job_record = job_records.first();

            if let Some(job_record) = job_record {
                log::info!("Running post migration job: sum_activities_into_agreement");

                let records: Vec<AgreementActivityRecord> = diesel::sql_query(
                    r#"
                    SELECT pag.id AS agreement_id,
                          pag.owner_id AS owner_id,
                          pag.role AS role,
                          pag.total_amount_paid AS total_amount_paid_agreement,
                          pac.total_amount_paid AS total_amount_paid_activity
                    FROM pay_agreement AS pag
                    JOIN pay_activity AS pac
                        ON pac.agreement_id = pag.id
                            AND pac.owner_id = pag.owner_id
                            AND pac.role = pag.role
                    ORDER BY agreement_id
                "#,
                )
                .load(conn)?;

                let mut current_idx: usize = 0;
                if let Some(first_record) = records.get(current_idx) {
                    let mut current_sum: BigDecimal = Zero::zero();
                    let mut current_agreement_id = first_record.agreement_id.clone();

                    while current_idx < records.len() {
                        let record = &records
                            .get(current_idx)
                            .expect("record has to be found on index");

                        current_sum +=
                            BigDecimal::from_str(&records[current_idx].total_amount_paid_activity)
                                .unwrap_or_default();

                        let write_total_sum = records
                            .get(current_idx + 1)
                            .map(|rec| rec.agreement_id != current_agreement_id.as_str())
                            .unwrap_or(true);
                        if write_total_sum {
                            current_sum +=
                                BigDecimal::from_str(&record.total_amount_paid_agreement)
                                    .unwrap_or_default();

                            diesel::sql_query(
                                r#"
                                UPDATE pay_agreement
                                SET total_amount_paid = $1
                                WHERE id = $2
                                    AND owner_id = $3
                                    AND role = $4
                            "#,
                            )
                            .bind::<Text, _>(current_sum.to_string())
                            .bind::<Text, _>(current_agreement_id)
                            .bind::<Text, _>(&record.owner_id)
                            .bind::<Text, _>(&record.role)
                            .execute(conn)?;
                            current_sum = Zero::zero();
                            current_agreement_id = records
                                .get(current_idx + 1)
                                .map(|rec| rec.agreement_id.clone())
                                .unwrap_or_default();
                        }
                        current_idx += 1;
                    }
                }

                log::info!(
                    "Post migration job: sum_activities_into_agreement done. Marking as done."
                );
                let marked = diesel::sql_query(
                    r#"
                        UPDATE pay_post_migration
                        SET done = STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW'),
                            result = 'ok'
                        WHERE job = ?
                    "#,
                )
                .bind::<Text, _>(JOB_NAME)
                .execute(conn)?;
                if marked != 1 {
                    log::error!(
                        "Post migration job: sum_activities_into_agreement not marked as done"
                    );
                }
            } else {
                log::info!("No post migration jobs to run");
            }
            Ok(())
        })
        .await
    }

    async fn send_batch_order_payments(
        &self,
        owner: NodeId,
        order_id: String,
    ) -> anyhow::Result<()> {
        let items = {
            let db_executor = self
                .db_executor
                .timeout_lock(DB_LOCK_TIMEOUT)
                .await
                .map_err(|err| {
                    ProcessPaymentsError::ProcessPaymentsError(format!(
                        "Db timeout lock when sending payments {err}"
                    ))
                })?;
            db_executor
                .as_dao::<BatchDao>()
                .get_unsent_batch_items(owner, order_id.clone())
                .await?
        };
        log::info!("got {} orders", items.len());
        let bus_id = driver_bus_id("erc20");
        for item in items {
            log::info!("sending: {:?}", &item);

            let deposit = item
                .deposit
                .map(|d| serde_json::from_str::<Deposit>(&d))
                .transpose()
                .map_err(|err| {
                    ProcessPaymentsError::ProcessPaymentsError(format!(
                        "Error parsing deposit: {err}"
                    ))
                })?;

            let payment_order_id = bus::service(&bus_id)
                .call(ScheduleDriverPayment::new(
                    item.amount.0,
                    item.payer_addr.clone(),
                    item.payee_addr.clone(),
                    item.platform.clone(),
                    deposit,
                    chrono::Utc::now(),
                ))
                .await??;
            {
                let db_executor = self
                    .db_executor
                    .timeout_lock(DB_LOCK_TIMEOUT)
                    .await
                    .map_err(|err| {
                        ProcessPaymentsError::ProcessPaymentsError(format!(
                            "Db timeout lock when sending payments {err}"
                        ))
                    })?;
                db_executor
                    .as_dao::<BatchDao>()
                    .batch_order_item_send(
                        order_id.clone(),
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

    async fn send_close_deposit_after_payments(
        &self,
        owner: NodeId,
        platform: String,
    ) -> Result<(), ProcessPaymentsError> {
        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await
            .map_err(|_| {
                ProcessPaymentsError::ProcessPaymentsError("Internal timeout".to_string())
            })?
            .driver(&platform, &owner.to_string(), AccountMode::SEND)
            .map_err(|err| {
                ProcessPaymentsError::ProcessPaymentsError(format!("Error getting driver: {err}"))
            })?;

        let allocations_to_close = {
            let db_executor = self
                .db_executor
                .timeout_lock(DB_LOCK_TIMEOUT)
                .await
                .map_err(|err| {
                    ProcessPaymentsError::ProcessPaymentsError(format!(
                        "Db timeout lock when sending payments {err}"
                    ))
                })?;

            db_executor
                .as_dao::<AllocationDao>()
                .get_allocations_to_close(owner, platform.clone())
                .await
                .map_err(|err| {
                    ProcessPaymentsError::ProcessPaymentsError(format!("db error: {}", err))
                })?
        };

        log::info!("got {} allocations to close", allocations_to_close.len());

        for allocation in allocations_to_close {
            let Some(deposit) = allocation.deposit else {
                return Err(ProcessPaymentsError::ProcessPaymentsError(format!(
                    "Deposit not found, it should be present on {:?}",
                    allocation
                )));
            };
            if platform != allocation.payment_platform {
                return Err(ProcessPaymentsError::ProcessPaymentsError(format!(
                    "Platform mismatch, expected: {}, got: {}",
                    allocation.payment_platform, platform
                )));
            }
            match driver_endpoint(&driver)
                .send(DriverReleaseDeposit {
                    platform: platform.clone(),
                    from: owner.to_string(),
                    deposit_contract: deposit.contract,
                    deposit_id: deposit.id,
                })
                .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    return Err(ProcessPaymentsError::ProcessPaymentsError(format!(
                        "Error releasing deposit 1: {e}"
                    )))
                }
                Err(e) => {
                    return Err(ProcessPaymentsError::ProcessPaymentsError(format!(
                        "Error releasing deposit 2: {e}"
                    )))
                }
            }

            {
                let db_executor = self
                    .db_executor
                    .timeout_lock(DB_LOCK_TIMEOUT)
                    .await
                    .map_err(|err| {
                        ProcessPaymentsError::ProcessPaymentsError(format!(
                            "Db timeout lock when closing allocation {err}"
                        ))
                    })?;
                db_executor
                    .as_dao::<AllocationDao>()
                    .mark_allocation_closing(allocation.allocation_id, owner)
                    .await
                    .map_err(|err| {
                        ProcessPaymentsError::ProcessPaymentsError(format!("db error: {}", err))
                    })?;
            }
        }
        Ok(())
    }

    fn db_batch_cycle_to_response(&self, el: DbPayBatchCycle) -> ProcessBatchCycleResponse {
        ProcessBatchCycleResponse {
            node_id: el.owner_id,
            platform: el.platform,
            interval: el.cycle_interval.map(|d| d.0.to_std().unwrap_or_default()),
            cron: el.cycle_cron,
            extra_payment_time: el.cycle_extra_pay_time.0.to_std().unwrap_or_default(),
            max_interval: el.cycle_max_interval.0.to_std().unwrap_or_default(),
            next_process: el.cycle_next_process.0,
            last_process: el.cycle_last_process.map(|d| d.0),
        }
    }

    pub async fn process_cycle_info(
        &self,
        msg: ProcessBatchCycleInfo,
    ) -> Result<ProcessBatchCycleResponse, ProcessBatchCycleError> {
        let db_executor = self
            .db_executor
            .timeout_lock(DB_LOCK_TIMEOUT)
            .await
            .map_err(|err| {
                ProcessBatchCycleError::ProcessBatchCycleError(format!(
                    "Db timeout lock when process payments {err}"
                ))
            })?;

        let el = db_executor
            .as_dao::<BatchCycleDao>()
            .get_or_insert_default(msg.node_id, msg.platform)
            .await
            .map_err(|err| {
                ProcessBatchCycleError::ProcessBatchCycleError(format!("db error: {}", err))
            })?;

        Ok(self.db_batch_cycle_to_response(el))
    }

    pub async fn process_cycle_set(
        &self,
        msg: ProcessBatchCycleSet,
    ) -> Result<ProcessBatchCycleResponse, ProcessBatchCycleError> {
        let db_executor = self
            .db_executor
            .timeout_lock(DB_LOCK_TIMEOUT)
            .await
            .map_err(|err| {
                ProcessBatchCycleError::ProcessBatchCycleError(format!(
                    "Db timeout lock when process payments {err}"
                ))
            })?;

        let el = db_executor
            .as_dao::<BatchCycleDao>()
            .create_or_update(
                msg.node_id,
                msg.platform.clone(),
                msg.interval
                    .map(|d| chrono::Duration::from_std(d).unwrap_or_default()),
                msg.cron,
                msg.safe_payout
                    .map(|d| chrono::Duration::from_std(d).unwrap_or_default()),
                msg.next_update,
            )
            .await
            .map_err(|err| {
                ProcessBatchCycleError::ProcessBatchCycleError(format!(
                    "create or update error: {err}"
                ))
            })?;

        self.batch_cycle_tasks
            .lock()
            .unwrap()
            .wake_owner_platform(msg.node_id, msg.platform.clone());
        Ok(self.db_batch_cycle_to_response(el))
    }

    pub async fn process_payments_now(
        &self,
        msg: ProcessPaymentsNow,
    ) -> Result<ProcessPaymentsNowResponse, ProcessPaymentsError> {
        {
            let operation_start = Instant::now();

            let mut resolve_time_ms = 0.0f64;
            let mut order_id = None;

            if !msg.skip_resolve {
                let db_executor = self
                    .db_executor
                    .timeout_lock(DB_LOCK_TIMEOUT)
                    .await
                    .map_err(|err| {
                        ProcessPaymentsError::ProcessPaymentsError(format!(
                            "Db timeout lock when process payments {err}"
                        ))
                    })?;
                db_executor
                    .as_dao::<BatchCycleDao>()
                    .mark_process_and_next(msg.node_id, msg.platform.clone())
                    .await
                    .map_err(|err| {
                        ProcessPaymentsError::ProcessPaymentsError(format!(
                            "Db error when mark_process_and_next payments {err}"
                        ))
                    })?;

                match db_executor
                    .as_dao::<BatchDao>()
                    .resolve(
                        msg.node_id,
                        msg.node_id.to_string(),
                        msg.platform.clone(),
                        Utc::now().sub(chrono::Duration::days(30)),
                    )
                    .await
                {
                    Ok(res) => {
                        resolve_time_ms = operation_start.elapsed().as_secs_f64() / 1000.0;
                        order_id = res;
                    }
                    Err(err) => {
                        log::error!("Error processing payments: {:?}", err);
                        return Err(ProcessPaymentsError::ProcessPaymentsError(format!(
                            "Error processing payments: {:?}",
                            err
                        )));
                    }
                };
            };
            let send_time_now = Instant::now();
            let mut send_time_ms = 0.0;
            if !msg.skip_send {
                if let Some(order_id) = order_id {
                    match self.send_batch_order_payments(msg.node_id, order_id).await {
                        Ok(()) => {}
                        Err(err) => {
                            log::error!("Error when sending payments {}", err);
                            return Err(ProcessPaymentsError::ProcessPaymentsError(format!(
                                "Error when sending payments {}",
                                err
                            )));
                        }
                    }
                    match self
                        .send_close_deposit_after_payments(msg.node_id, msg.platform.clone())
                        .await
                    {
                        Ok(()) => {
                            send_time_ms = send_time_now.elapsed().as_secs_f64() / 1000.0;
                        }
                        Err(err) => {
                            log::error!("Error when closing deposits {}", err);
                            return Err(ProcessPaymentsError::ProcessPaymentsError(format!(
                                "Error when closing deposits {}",
                                err
                            )));
                        }
                    }
                }
            };
            Ok(ProcessPaymentsNowResponse {
                resolve_time_ms,
                send_time_ms,
            })
        }
    }

    pub async fn notify_payment(&self, msg: NotifyPayment) -> Result<(), NotifyPaymentError> {
        let driver = msg.driver;
        let payment_platform = msg.platform;
        let payer_addr = msg.sender.clone();
        let payee_addr = msg.recipient;

        let payer_id = NodeId::from_str(&msg.sender)
            .map_err(|err| NotifyPaymentError::Other(format!("Invalid payer address: {err}")))?;
        let payee_id = NodeId::from_str(&payee_addr)
            .map_err(|err| NotifyPaymentError::Other(format!("Invalid payee address: {err}")))?;

        if payer_addr == payee_addr {
            log::warn!(
                "Payer and payee addresses are the same: {} - skip notification",
                payer_addr
            );
            return Ok(());
        }

        let payment_id: String;

        let payment: Payment = {
            let db_executor = self.db_executor.timeout_lock(DB_LOCK_TIMEOUT).await?;

            let order_items = db_executor
                .as_dao::<BatchDao>()
                .get_batch_order_items_by_payment_id(msg.payment_id, payer_id)
                .await?;

            let mut activity_payments = vec![];
            let mut agreement_payments = vec![];

            for order_item in order_items.iter() {
                let order_documents = match db_executor
                    .as_dao::<BatchDao>()
                    .get_batch_items(
                        payer_id,
                        BatchItemFilter {
                            order_id: Some(order_item.order_id.clone()),
                            payee_addr: Some(order_item.payee_addr.clone()),
                            ..Default::default()
                        },
                    )
                    .await
                {
                    Ok(items) => items,
                    Err(e) => {
                        return Err(NotifyPaymentError::Other(format!(
                            "Error getting batch items: {e}"
                        )));
                    }
                };

                db_executor
                    .as_dao::<BatchDao>()
                    .batch_order_item_paid(
                        order_item.order_id.clone(),
                        payer_id,
                        order_item.payee_addr.clone(),
                        order_item.allocation_id.clone(),
                    )
                    .await?;
                for order in order_documents.iter() {
                    let amount = order.amount.clone().into();
                    match order.activity_id.clone() {
                        Some(activity_id) => activity_payments.push(ActivityPayment {
                            activity_id,
                            amount,
                            allocation_id: None,
                        }),
                        None => agreement_payments.push(AgreementPayment {
                            agreement_id: order.agreement_id.clone(),
                            amount,
                            allocation_id: None,
                        }),
                    }
                }
            }

            let payment_dao: PaymentDao = db_executor.as_dao();

            payment_id = payment_dao
                .create_new(
                    payer_id,
                    payee_id,
                    payer_addr,
                    payee_addr,
                    payment_platform.clone(),
                    msg.amount.clone(),
                    msg.confirmation.confirmation,
                    activity_payments,
                    agreement_payments,
                )
                .await?;

            let signed_payment = payment_dao
                .get(payment_id.clone(), payer_id)
                .await?
                .unwrap();
            signed_payment.payload
        };

        let signature_canonical = driver_endpoint(&driver)
            .send(driver::SignPaymentCanonicalized(payment.clone()))
            .await??;
        let signature = driver_endpoint(&driver)
            .send(driver::SignPayment(payment.clone()))
            .await??;

        counter!("payment.amount.sent", ya_metrics::utils::cryptocurrency_to_u64(&msg.amount), "platform" => payment_platform);
        // This is unconditional because at this point the invoice *has been paid*.
        // Whether the provider was correctly notified of this fact is another matter.
        counter!("payment.invoices.requestor.paid", 1);
        let msg = SendPayment::new(payment.clone(), signature);
        let msg_with_bytes = SendSignedPayment::new(payment.clone(), signature_canonical);

        let db_executor = Arc::clone(&self.db_executor);

        tokio::task::spawn_local(
            async move {
                let send_result =
                    Self::send_to_gsb(payer_id, payee_id, msg_with_bytes.clone()).await;

                let mark_sent = match send_result {
                    Ok(_) => true,
                    // If sending SendPaymentWithBytes is not supported then use SendPayment as fallback.
                    Err(PaymentSendToGsbError::NotSupported) => {
                        match Self::send_to_gsb(payer_id, payee_id, msg).await {
                            Ok(_) => true,
                            Err(PaymentSendToGsbError::Rejected) => true,
                            Err(PaymentSendToGsbError::Failed) => false,
                            Err(PaymentSendToGsbError::NotSupported) => false,
                        }
                    }
                    Err(_) => false,
                };

                let db_executor = db_executor.timeout_lock(DB_LOCK_TIMEOUT).await?;

                let payment_dao: PaymentDao = db_executor.as_dao();
                let sync_dao: SyncNotifsDao = db_executor.as_dao();

                // Always add new type of signature. Compatibility is for older Provider nodes only.
                payment_dao
                    .add_signature(
                        payment_id.clone(),
                        msg_with_bytes.signature.clone(),
                        msg_with_bytes.signed_bytes.clone(),
                    )
                    .await?;

                if mark_sent {
                    payment_dao.mark_sent(payment_id).await?;
                } else {
                    sync_dao.upsert(payee_id).await?;
                    SYNC_NOTIFS_NOTIFY.notify_one();
                    log::debug!("Failed to call SendPayment on [{payee_id}]");
                }

                anyhow::Ok(())
            }
            .inspect_err(|e| log::error!("Notify payment task failed: {e}")),
        );

        Ok(())
    }

    async fn send_to_gsb<T: RpcMessage + Unpin>(
        payer_id: NodeId,
        payee_id: NodeId,
        msg: T,
    ) -> Result<(), PaymentSendToGsbError> {
        ya_net::from(payer_id)
            .to(payee_id)
            .service(BUS_ID)
            .call(msg)
            .map(|res| match res {
                Ok(Ok(_)) => Ok(()),
                Err(e) if e.to_string().contains("endpoint address not found") => {
                    Err(PaymentSendToGsbError::NotSupported)
                }
                Err(err) => {
                    log::error!("Error sending payment message to provider: {:?}", err);
                    Err(PaymentSendToGsbError::Failed)
                }
                Ok(Err(err)) => {
                    log::error!("Provider rejected payment: {:?}", err);
                    Err(PaymentSendToGsbError::Rejected)
                }
            })
            .await
    }

    pub async fn collect_payments(&self, msg: CollectPayments) -> Result<(), GenericError> {
        Ok(())
    }

    pub async fn verify_payment(
        &self,
        payment: Payment,
        signature: Vec<u8>,
        canonical: Option<Vec<u8>>,
    ) -> Result<(), VerifyPaymentError> {
        // TODO: Split this into smaller functions
        let platform = payment.payment_platform.clone();
        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(
                &payment.payment_platform,
                &payment.payee_addr,
                AccountMode::RECV,
            )?;

        if !driver_endpoint(&driver)
            .send(driver::VerifySignature::new(
                payment.clone(),
                signature.clone(),
                canonical.clone(),
            ))
            .await??
        {
            return Err(VerifyPaymentError::InvalidSignature);
        }

        let confirmation = match base64::decode(&payment.details) {
            Ok(confirmation) => PaymentConfirmation { confirmation },
            Err(e) => return Err(VerifyPaymentError::ConfirmationEncoding),
        };
        let details: PaymentDetails = driver_endpoint(&driver)
            .send(driver::VerifyPayment::new(
                confirmation.clone(),
                platform.clone(),
                payment.clone(),
            ))
            .await??;

        // Verify if amount declared in message matches actual amount transferred on blockchain
        if details.amount < payment.amount {
            return VerifyPaymentError::amount(&details.amount, &payment.amount);
        }

        // Verify if payment shares for agreements and activities sum up to the total amount
        let agreement_sum = payment.agreement_payments.iter().map(|p| &p.amount).sum();
        let activity_sum = payment.activity_payments.iter().map(|p| &p.amount).sum();
        if details.amount < (&agreement_sum + &activity_sum) {
            return VerifyPaymentError::shares(&details.amount, &agreement_sum, &activity_sum);
        }

        let payee_id = payment.payee_id;
        let payer_id = payment.payer_id;
        let payee_addr = &payment.payee_addr;
        let payer_addr = &payment.payer_addr;

        // Verify recipient address
        if &details.recipient != payee_addr {
            return VerifyPaymentError::recipient(payee_addr, &details.recipient);
        }
        if &details.sender != payer_addr {
            return VerifyPaymentError::sender(payer_addr, &details.sender);
        }

        {
            let db_executor = self.db_executor.timeout_lock(DB_LOCK_TIMEOUT).await?;

            // Verify agreement payments
            let agreement_dao: AgreementDao = db_executor.as_dao();
            for agreement_payment in payment.agreement_payments.iter() {
                let agreement_id = &agreement_payment.agreement_id;
                let agreement = agreement_dao.get(agreement_id.clone(), payee_id).await?;
                if agreement_payment.amount == BigDecimal::zero() {
                    return VerifyPaymentError::agreement_zero_amount(agreement_id);
                }
                match agreement {
                    None => return VerifyPaymentError::agreement_not_found(agreement_id),
                    Some(agreement) if &agreement.payee_addr != payee_addr => {
                        return VerifyPaymentError::agreement_payee(&agreement, payee_addr);
                    }
                    Some(agreement) if &agreement.payer_addr != payer_addr => {
                        return VerifyPaymentError::agreement_payer(&agreement, payer_addr);
                    }
                    Some(agreement) if agreement.payment_platform != payment.payment_platform => {
                        return VerifyPaymentError::agreement_platform(
                            &agreement,
                            &payment.payment_platform,
                        );
                    }
                    _ => (),
                }
            }

            // Verify activity payments
            let activity_dao: ActivityDao = db_executor.as_dao();
            for activity_payment in payment.activity_payments.iter() {
                let activity_id = &activity_payment.activity_id;
                if activity_payment.amount == BigDecimal::zero() {
                    return VerifyPaymentError::activity_zero_amount(activity_id);
                }
                let activity = activity_dao.get(activity_id.clone(), payee_id).await?;
                match activity {
                    None => return VerifyPaymentError::activity_not_found(activity_id),
                    Some(activity) if &activity.payee_addr != payee_addr => {
                        return VerifyPaymentError::activity_payee(&activity, payee_addr);
                    }
                    Some(activity) if &activity.payer_addr != payer_addr => {
                        return VerifyPaymentError::activity_payer(&activity, payer_addr);
                    }
                    _ => (),
                }
            }

            // Verify totals for all agreements and activities with the same confirmation
            let payment_dao: PaymentDao = db_executor.as_dao();
            let shared_payments = payment_dao
                .get_for_confirmation(confirmation.confirmation, Role::Provider)
                .await?;
            let other_payment_total = shared_payments
                .iter()
                .map(|payment| {
                    let agreement_total = payment
                        .agreement_payments
                        .iter()
                        .map(|ap| &ap.amount)
                        .sum::<BigDecimal>();

                    let activity_total = payment
                        .activity_payments
                        .iter()
                        .map(|ap| &ap.amount)
                        .sum::<BigDecimal>();

                    agreement_total + activity_total
                })
                .sum::<BigDecimal>();

            let all_payment_total = &other_payment_total + agreement_sum + activity_sum;
            if all_payment_total > details.amount {
                return VerifyPaymentError::overspending(&details.amount, &all_payment_total);
            }

            // Insert payment into database (this operation creates and updates all related entities)
            if canonical.is_none() {
                payment_dao
                    .insert_received(payment, payee_id, None, None)
                    .await?;
            } else {
                payment_dao
                    .insert_received(payment, payee_id, Some(signature), canonical)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn get_status(
        &self,
        platform: String,
        address: String,
    ) -> Result<GetAccountBalanceResult, GetStatusError> {
        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(&platform, &address, AccountMode::empty())?;
        let status = driver_endpoint(&driver)
            .send(driver::GetAccountBalance::new(address, platform))
            .await??;
        Ok(status)
    }

    pub async fn get_rpc_endpoints_info(
        &self,
        platform: String,
        address: String,
        network: Option<String>,
        verify: bool,
        resolve: bool,
        no_wait: bool,
    ) -> Result<GetRpcEndpointsResult, GetStatusError> {
        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(&platform, &address, AccountMode::empty())?;
        let res = driver_endpoint(&driver)
            .send(driver::GetRpcEndpoints {
                network,
                verify,
                resolve,
                no_wait,
            })
            .await??;
        Ok(res)
    }

    pub async fn validate_allocation(
        &self,
        platform: String,
        address: String,
        amount: BigDecimal,
        timeout: Option<DateTime<Utc>>,
        deposit: Option<Deposit>,
        new_allocation: bool,
    ) -> Result<ValidateAllocationResult, ValidateAllocationError> {
        if self.in_shutdown.load(Ordering::SeqCst) {
            return Err(ValidateAllocationError::Shutdown);
        }

        if let Some(requested_timeout) = timeout {
            if requested_timeout < chrono::Utc::now() {
                return Ok(ValidateAllocationResult::TimeoutPassed { requested_timeout });
            }
        }

        let (active_allocations, past_allocations) = {
            let db = self.db_executor.timeout_lock(DB_LOCK_TIMEOUT).await?;
            let dao = db.as_dao::<AllocationDao>();

            let active = dao
                .get_for_address(platform.clone(), address.clone(), Some(false))
                .await?;
            let past = dao
                .get_for_address(platform.clone(), address.clone(), Some(true))
                .await?;

            (active, past)
        };

        let driver = self
            .registry
            .timeout_read(REGISTRY_LOCK_TIMEOUT)
            .await?
            .driver(&platform, &address, AccountMode::empty())?;
        let msg = ValidateAllocation {
            address,
            platform,
            amount,
            timeout,
            deposit,
            active_allocations,
            past_allocations,
            new_allocation,
        };
        let result = driver_endpoint(&driver).send(msg).await??;
        Ok(result)
    }

    /// This function releases allocations.
    /// When `bool` is `true` all existing allocations are released immediately.
    /// For `false` each allocation timestamp is respected.
    pub async fn release_allocations(&self, force: bool) {
        // keep this lock alive for the entirety of this function for now
        let db_executor = match self.db_executor.timeout_lock(DB_LOCK_TIMEOUT).await {
            Ok(db) => db,
            Err(_) => {
                log::error!("Timed out waiting for db lock");
                return;
            }
        };

        let db = Data::new(db_executor.clone());
        let active_allocations = db
            .clone()
            .as_dao::<AllocationDao>()
            .get_filtered(None, None, None, None, None, Some(false))
            .await;

        if force {
            log::info!("Releasing all active allocations...");
        } else {
            log::info!("Releasing expired allocations...");
        }

        match active_allocations {
            Ok(allocations) => {
                if !allocations.is_empty() {
                    for allocation in allocations {
                        if force {
                            forced_release_allocation(
                                db.clone(),
                                allocation.allocation_id.clone(),
                                NodeId::from_str(&allocation.address.clone())
                                    .map_err(|e| {
                                        GenericError::new(format!(
                                            "Invalid node id: {} {}",
                                            allocation.address.clone(),
                                            e
                                        ))
                                    })
                                    .unwrap_or_else(|err| {
                                        log::error!(
                                            "Invalid node id: {} {}",
                                            allocation.address.clone(),
                                            err
                                        );
                                        NodeId::default()
                                    }),
                            )
                            .await
                        } else {
                            release_allocation_after(
                                db.clone(),
                                allocation.allocation_id.clone(),
                                allocation.timeout,
                                NodeId::from_str(&allocation.address.clone())
                                    .map_err(|e| {
                                        GenericError::new(format!(
                                            "Invalid node id: {} {}",
                                            allocation.address.clone(),
                                            e
                                        ))
                                    })
                                    .unwrap_or_else(|err| {
                                        log::error!(
                                            "Invalid node id: {} {}",
                                            allocation.address.clone(),
                                            err
                                        );
                                        NodeId::default()
                                    }),
                            )
                            .await
                        }
                    }
                } else {
                    log::info!("No allocations found to be released.")
                }
            }
            Err(e) => {
                log::error!("Allocations release failed. Restart yagna to retry allocations release. Db error occurred: {}.", e);
            }
        }
    }

    pub async fn shut_down(
        &self,
        timeout: Duration,
    ) -> impl futures::Future<Output = ()> + 'static {
        self.in_shutdown.store(true, Ordering::SeqCst);

        let driver_shutdown_futures: Vec<_> = {
            let registry = self
                .registry
                .timeout_read(REGISTRY_LOCK_TIMEOUT)
                .await
                .expect("Can't initiate payment shutdown: registry lock timed out");

            registry
                .iter_drivers()
                .map(|driver| shut_down_driver(driver, timeout))
                .collect()
        };
        futures::future::join_all(driver_shutdown_futures).map(|_| ())
    }
}

fn shut_down_driver(
    driver: &str,
    timeout: Duration,
) -> impl futures::Future<Output = ()> + 'static {
    let driver = driver.to_string();
    let endpoint = driver_endpoint(&driver);
    let shutdown_msg = ShutDown::new(timeout);
    async move {
        log::info!("Shutting down driver '{}'... timeout={:?}", driver, timeout);
        match endpoint.call(shutdown_msg).await {
            Ok(Ok(_)) => log::info!("Driver '{}' shut down successfully.", driver),
            err => log::error!("Error shutting down driver '{}': {:?}", driver, err),
        }
    }
}
