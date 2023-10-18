use crate::dao::{DebitNoteDao, InvoiceDao, PaymentDao};
use crate::{dao::SyncNotifsDao, processor::PaymentProcessor};
use chrono::Utc;
use futures::lock::Mutex;
use futures::prelude::*;
use metrics::counter;
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};
use tokio_util::task::LocalPoolHandle;
use ya_client_model::payment::Acceptance;
use ya_client_model::NodeId;
use ya_core_model::driver::{driver_bus_id, SignPayment};
use ya_core_model::payment::local::{GenericError, BUS_ID as PAYMENT_BUS_ID};
use ya_core_model::payment::public::{AcceptDebitNote, AcceptInvoice, PaymentSync, SendPayment};
use ya_core_model::{identity, payment};
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::{self, service, ServiceBinder};
use ya_service_bus::RpcEndpoint;

const SYNC_NOTIF_DELAY_0: Duration = Duration::from_secs(30);
const SYNC_NOTIF_RATIO: u32 = 6;
const SYNC_NOTIF_MAX_RETRIES: u32 = 7;

pub fn bind_service(db: &DbExecutor, processor: PaymentProcessor) {
    log::debug!("Binding payment service to service bus");

    let processor = Arc::new(Mutex::new(processor));
    local::bind_service(db, processor.clone());
    public::bind_service(db, processor);

    log::debug!("Successfully bound payment service to service bus");
}

async fn payment_sync(db: &DbExecutor, peer_id: NodeId) -> anyhow::Result<PaymentSync> {
    let payment_dao: PaymentDao = db.as_dao();
    let invoice_dao: InvoiceDao = db.as_dao();
    let debit_note_dao: DebitNoteDao = db.as_dao();

    let mut payments = Vec::default();
    for payment in payment_dao.list_unsent(peer_id).await? {
        let platform_components = payment.payment_platform.split('-').collect::<Vec<_>>();
        let driver = &platform_components[0];

        let signature = typed::service(driver_bus_id(driver))
            .send(SignPayment(payment.clone()))
            .await??;

        payments.push(SendPayment::new(payment, signature));
    }

    let mut invoice_accepts = Vec::default();
    for invoice in invoice_dao.unsent_accepted(peer_id).await? {
        invoice_accepts.push(AcceptInvoice::new(
            invoice.invoice_id,
            Acceptance {
                total_amount_accepted: invoice.amount,
                allocation_id: String::new(),
            },
            peer_id,
        ));
    }

    let mut debit_note_accepts = Vec::default();
    for debit_note in debit_note_dao.unsent_accepted(peer_id).await? {
        debit_note_accepts.push(AcceptDebitNote::new(
            debit_note.debit_note_id,
            Acceptance {
                total_amount_accepted: debit_note.total_amount_due,
                allocation_id: String::new(),
            },
            peer_id,
        ));
    }

    Ok(PaymentSync {
        payments: payments,
        invoice_accepts,
        debit_note_accepts,
    })
}

async fn mark_all_sent(db: &DbExecutor, msg: PaymentSync) -> anyhow::Result<()> {
    let payment_dao: PaymentDao = db.as_dao();
    let invoice_dao: InvoiceDao = db.as_dao();
    let debit_note_dao: DebitNoteDao = db.as_dao();

    for payment_send in msg.payments {
        payment_dao
            .mark_sent(payment_send.payment.payment_id)
            .await?;
    }

    for invoice_accept in msg.invoice_accepts {
        invoice_dao
            .mark_accept_sent(invoice_accept.invoice_id, invoice_accept.issuer_id)
            .await?;
    }

    for debit_note_accept in msg.debit_note_accepts {
        debit_note_dao
            .mark_accept_sent(debit_note_accept.debit_note_id, debit_note_accept.issuer_id)
            .await?;
    }

    Ok(())
}

async fn send_sync_notifs(db: &DbExecutor) -> anyhow::Result<()> {
    let dao: SyncNotifsDao = db.as_dao();

    let exp_backoff = |n| SYNC_NOTIF_DELAY_0 * SYNC_NOTIF_RATIO.pow(n);
    let cutoff = Utc::now();

    let default_identity = typed::service(identity::BUS_ID)
        .call(ya_core_model::identity::Get::ByDefault {})
        .await??
        .ok_or_else(|| anyhow::anyhow!("No default identity"))?
        .node_id;

    let peers_to_notify = dao
        .list()
        .await?
        .into_iter()
        .filter(|entry| {
            let next_deadline = entry.timestamp + exp_backoff(entry.retries as _);
            next_deadline.and_utc() < cutoff
        })
        .map(|entry| entry.id)
        .collect::<Vec<_>>();

    for peer in peers_to_notify {
        let msg = payment_sync(db, peer).await?;

        let result = ya_net::from(default_identity)
            .to(peer)
            .service(payment::public::BUS_ID)
            .call(msg.clone())
            .await;

        if matches!(&result, Ok(Ok(_))) {
            mark_all_sent(db, msg).await?;
            dao.drop(peer).await?;
        } else {
            dao.increment_retry(peer, cutoff.naive_utc()).await?;
        }
    }

    Ok(())
}

fn send_sync_notifs_job(db: DbExecutor) {
    let pool = LocalPoolHandle::new(5);
    pool.spawn_pinned(|| async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs_f32(30.0)).await;
            if let Err(e) = send_sync_notifs(&db).await {
                log::error!("PaymentSyncNeeded sendout job failed: {e}");
            } else {
                log::trace!("PaymentSyncNeeded sendout job done");
            }
        }
    });
}

mod local {
    use super::*;
    use crate::dao::*;
    use chrono::NaiveDateTime;
    use std::collections::BTreeMap;
    use ya_client_model::payment::{Account, DocumentStatus, DriverDetails, DriverStatusProperty};
    use ya_core_model::{
        driver::{driver_bus_id, DriverStatus, DriverStatusError},
        payment::local::*,
    };
    use ya_persistence::types::Role;

    pub fn bind_service(db: &DbExecutor, processor: Arc<Mutex<PaymentProcessor>>) {
        log::debug!("Binding payment local service to service bus");

        ServiceBinder::new(BUS_ID, db, processor)
            .bind_with_processor(schedule_payment)
            .bind_with_processor(register_driver)
            .bind_with_processor(unregister_driver)
            .bind_with_processor(register_account)
            .bind_with_processor(unregister_account)
            .bind_with_processor(notify_payment)
            .bind_with_processor(get_status)
            .bind_with_processor(get_invoice_stats)
            .bind_with_processor(get_accounts)
            .bind_with_processor(validate_allocation)
            .bind_with_processor(release_allocations)
            .bind_with_processor(get_drivers)
            .bind_with_processor(payment_driver_status)
            .bind_with_processor(shut_down);

        send_sync_notifs_job(db.clone());

        // Initialize counters to 0 value. Otherwise they won't appear on metrics endpoint
        // until first change to value will be made.
        counter!("payment.invoices.requestor.accepted", 0);
        counter!("payment.invoices.requestor.accepted.call", 0);
        counter!("payment.invoices.requestor.received", 0);
        counter!("payment.invoices.requestor.received.call", 0);
        counter!("payment.invoices.requestor.cancelled", 0);
        counter!("payment.invoices.requestor.cancelled.call", 0);
        counter!("payment.invoices.requestor.paid", 0);
        counter!("payment.debit_notes.requestor.accepted", 0);
        counter!("payment.debit_notes.requestor.accepted.call", 0);
        counter!("payment.debit_notes.requestor.received", 0);
        counter!("payment.debit_notes.requestor.received.call", 0);
        counter!("payment.debit_notes.provider.issued", 0);
        counter!("payment.debit_notes.provider.sent", 0);
        counter!("payment.debit_notes.provider.sent.call", 0);
        counter!("payment.debit_notes.provider.accepted", 0);
        counter!("payment.debit_notes.provider.accepted.call", 0);
        counter!("payment.invoices.provider.issued", 0);
        counter!("payment.invoices.provider.sent", 0);
        counter!("payment.invoices.provider.sent.call", 0);
        counter!("payment.invoices.provider.cancelled", 0);
        counter!("payment.invoices.provider.cancelled.call", 0);
        counter!("payment.invoices.provider.paid", 0);
        counter!("payment.invoices.provider.accepted", 0);
        counter!("payment.invoices.provider.accepted.call", 0);
        counter!("payment.invoices.requestor.not-enough-funds", 0);

        counter!("payment.amount.received", 0, "platform" => "erc20-rinkeby-tglm");
        counter!("payment.amount.received", 0, "platform" => "erc20-mainnet-glm");

        counter!("payment.amount.sent", 0, "platform" => "erc20-rinkeby-tglm");
        counter!("payment.amount.sent", 0, "platform" => "erc20-mainnet-glm");

        log::debug!("Successfully bound payment local service to service bus");
    }

    async fn schedule_payment(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: SchedulePayment,
    ) -> Result<(), GenericError> {
        processor.lock().await.schedule_payment(msg).await?;
        Ok(())
    }

    async fn register_driver(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: RegisterDriver,
    ) -> Result<(), RegisterDriverError> {
        processor.lock().await.register_driver(msg).await
    }

    async fn unregister_driver(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: UnregisterDriver,
    ) -> Result<(), NoError> {
        processor.lock().await.unregister_driver(msg).await;
        Ok(())
    }

    async fn register_account(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: RegisterAccount,
    ) -> Result<(), RegisterAccountError> {
        processor.lock().await.register_account(msg).await
    }

    async fn unregister_account(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: UnregisterAccount,
    ) -> Result<(), NoError> {
        processor.lock().await.unregister_account(msg).await;
        Ok(())
    }

    async fn get_accounts(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: GetAccounts,
    ) -> Result<Vec<Account>, GenericError> {
        Ok(processor.lock().await.get_accounts().await)
    }

    async fn notify_payment(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: NotifyPayment,
    ) -> Result<(), GenericError> {
        processor.lock().await.notify_payment(msg).await?;
        Ok(())
    }

    async fn get_status(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        _caller: String,
        msg: GetStatus,
    ) -> Result<StatusResult, GenericError> {
        log::info!("get status: {:?}", msg);
        let GetStatus {
            address,
            driver,
            network,
            token,
            after_timestamp,
        } = msg;

        let (network, network_details) = processor
            .lock()
            .await
            .get_network(driver.clone(), network)
            .await
            .map_err(GenericError::new)?;
        let token = token.unwrap_or_else(|| network_details.default_token.clone());
        let after_timestamp = NaiveDateTime::from_timestamp_opt(after_timestamp, 0)
            .expect("Failed on out-of-range number of seconds");
        let platform = match network_details.tokens.get(&token) {
            Some(platform) => platform.clone(),
            None => {
                return Err(GenericError::new(format!(
                    "Unsupported token. driver={} network={} token={}",
                    driver, network, token
                )));
            }
        };

        let incoming_fut = async {
            db.as_dao::<AgreementDao>()
                .incoming_transaction_summary(platform.clone(), address.clone(), after_timestamp)
                .await
        }
        .map_err(GenericError::new);

        let outgoing_fut = async {
            db.as_dao::<AgreementDao>()
                .outgoing_transaction_summary(platform.clone(), address.clone(), after_timestamp)
                .await
        }
        .map_err(GenericError::new);

        let reserved_fut = async {
            db.as_dao::<AllocationDao>()
                .total_remaining_allocation(platform.clone(), address.clone(), after_timestamp)
                .await
        }
        .map_err(GenericError::new);

        let amount_fut = async {
            processor
                .lock()
                .await
                .get_status(platform.clone(), address.clone())
                .await
        }
        .map_err(GenericError::new);

        let gas_amount_fut = async {
            processor
                .lock()
                .await
                .get_gas_balance(platform.clone(), address.clone())
                .await
        }
        .map_err(GenericError::new);

        let (incoming, outgoing, amount, gas, reserved) = future::try_join5(
            incoming_fut,
            outgoing_fut,
            amount_fut,
            gas_amount_fut,
            reserved_fut,
        )
        .await?;

        Ok(StatusResult {
            amount,
            reserved,
            outgoing,
            incoming,
            driver,
            network,
            token,
            gas,
        })
    }

    async fn get_invoice_stats(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        _caller: String,
        msg: GetInvoiceStats,
    ) -> Result<InvoiceStats, GenericError> {
        let stats: BTreeMap<(Role, DocumentStatus), StatValue> = async {
            db.as_dao::<InvoiceDao>()
                .last_invoice_stats(msg.node_id, msg.since)
                .await
        }
        .map_err(GenericError::new)
        .await?;
        let mut output_stats = InvoiceStats::default();

        fn aggregate(
            iter: impl Iterator<Item = (DocumentStatus, StatValue)>,
        ) -> InvoiceStatusNotes {
            let mut notes = InvoiceStatusNotes::default();
            for (status, value) in iter {
                match status {
                    DocumentStatus::Issued => notes.issued += value,
                    DocumentStatus::Received => notes.received += value,
                    DocumentStatus::Accepted => notes.accepted += value,
                    DocumentStatus::Rejected => notes.rejected += value,
                    DocumentStatus::Failed => notes.failed += value,
                    DocumentStatus::Settled => notes.settled += value,
                    DocumentStatus::Cancelled => notes.cancelled += value,
                }
            }
            notes
        }

        if msg.provider {
            output_stats.provider = aggregate(
                stats
                    .iter()
                    .filter(|((role, _), _)| matches!(role, Role::Provider))
                    .map(|((_, status), value)| (*status, value.clone())),
            );
        }
        if msg.requestor {
            output_stats.requestor = aggregate(
                stats
                    .iter()
                    .filter(|((role, _), _)| matches!(role, Role::Requestor))
                    .map(|((_, status), value)| (*status, value.clone())),
            );
        }
        Ok(output_stats)
    }

    async fn validate_allocation(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: ValidateAllocation,
    ) -> Result<bool, ValidateAllocationError> {
        Ok(processor
            .lock()
            .await
            .validate_allocation(msg.platform, msg.address, msg.amount)
            .await?)
    }

    async fn release_allocations(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        _caller: String,
        msg: ReleaseAllocations,
    ) -> Result<(), GenericError> {
        processor.lock().await.release_allocations(true).await;
        Ok(())
    }

    async fn get_drivers(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        _caller: String,
        msg: GetDrivers,
    ) -> Result<HashMap<String, DriverDetails>, NoError> {
        Ok(processor.lock().await.get_drivers().await)
    }

    async fn payment_driver_status(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        _caller: String,
        msg: PaymentDriverStatus,
    ) -> Result<Vec<DriverStatusProperty>, PaymentDriverStatusError> {
        let drivers = match &msg.driver {
            Some(driver) => vec![driver.clone()],
            None => {
                #[allow(clippy::iter_kv_map)]
                // Unwrap is provably safe because NoError can't be instanciated
                match service(PAYMENT_BUS_ID).call(GetDrivers {}).await {
                    Ok(drivers) => drivers,
                    Err(e) => return Err(PaymentDriverStatusError::Internal(e.to_string())),
                }
                .unwrap()
                .into_iter()
                .map(|(driver_name, _)| driver_name)
                .collect()
            }
        };

        let mut status_props = Vec::new();
        for driver in drivers {
            let result = match service(driver_bus_id(&driver))
                .call(DriverStatus {
                    network: msg.network.clone(),
                })
                .await
            {
                Ok(result) => result,
                Err(e) => return Err(PaymentDriverStatusError::NoDriver(driver)),
            };

            match result {
                Ok(status) => status_props.extend(status),
                Err(DriverStatusError::NetworkNotFound(network)) => {
                    return Err(PaymentDriverStatusError::NoNetwork(network))
                }
            }
        }

        Ok(status_props)
    }

    async fn shut_down(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender: String,
        msg: ShutDown,
    ) -> Result<(), GenericError> {
        // It's crucial to drop the lock on processor (hence assigning the future to a variable).
        // Otherwise, we won't be able to handle calls to `notify_payment` sent by drivers during shutdown.
        let shutdown_future = processor.lock().await.shut_down(msg.timeout);
        shutdown_future.await;
        Ok(())
    }
}

mod public {
    use super::*;

    use crate::dao::*;
    use crate::error::DbError;
    use crate::utils::*;

    // use crate::error::processor::VerifyPaymentError;
    use ya_client_model::payment::*;
    use ya_core_model::payment::public::*;
    use ya_persistence::types::Role;

    pub fn bind_service(db: &DbExecutor, processor: Arc<Mutex<PaymentProcessor>>) {
        log::debug!("Binding payment public service to service bus");

        ServiceBinder::new(BUS_ID, db, processor)
            .bind(send_debit_note)
            .bind(accept_debit_note)
            .bind(reject_debit_note)
            .bind(cancel_debit_note)
            .bind(send_invoice)
            .bind(accept_invoice)
            .bind(reject_invoice)
            .bind(cancel_invoice)
            .bind_with_processor(send_payment)
            .bind_with_processor(sync_payment);

        log::debug!("Successfully bound payment public service to service bus");
    }

    // ************************** DEBIT NOTE **************************

    async fn send_debit_note(
        db: DbExecutor,
        sender_id: String,
        msg: SendDebitNote,
    ) -> Result<Ack, SendError> {
        let debit_note = msg.0;
        let debit_note_id = debit_note.debit_note_id.clone();
        let activity_id = debit_note.activity_id.clone();
        let agreement_id = debit_note.agreement_id.clone();

        log::debug!(
            "Got SendDebitNote [{}] from Node [{}].",
            debit_note_id,
            sender_id
        );
        counter!("payment.debit_notes.requestor.received.call", 1);

        let agreement = match get_agreement(
            agreement_id.clone(),
            ya_client_model::market::Role::Requestor,
        )
        .await
        {
            Err(e) => {
                return Err(SendError::ServiceError(e.to_string()));
            }
            Ok(None) => {
                return Err(SendError::BadRequest(format!(
                    "Agreement {} not found",
                    debit_note.agreement_id
                )));
            }
            Ok(Some(agreement)) => agreement,
        };

        let offeror_id = agreement.offer.provider_id.to_string();
        let issuer_id = debit_note.issuer_id.to_string();
        if sender_id != offeror_id || sender_id != issuer_id {
            return Err(SendError::BadRequest("Invalid sender node ID".to_owned()));
        }

        let node_id = *agreement.requestor_id();
        match async move {
            db.as_dao::<AgreementDao>()
                .create_if_not_exists(agreement, node_id, Role::Requestor)
                .await?;
            db.as_dao::<ActivityDao>()
                .create_if_not_exists(activity_id.clone(), node_id, Role::Requestor, agreement_id)
                .await?;
            db.as_dao::<DebitNoteDao>()
                .insert_received(debit_note)
                .await?;

            log::info!(
                "DebitNote [{debit_note_id}] for Activity [{activity_id}] received from node [{issuer_id}]."
            );
            counter!("payment.debit_notes.requestor.received", 1);
            Ok(())
        }
        .await
        {
            Ok(_) => Ok(Ack {}),
            Err(DbError::Query(e)) => Err(SendError::BadRequest(e)),
            Err(e) => Err(SendError::ServiceError(e.to_string())),
        }
    }

    async fn accept_debit_note(
        db: DbExecutor,
        sender_id: String,
        msg: AcceptDebitNote,
    ) -> Result<Ack, AcceptRejectError> {
        let debit_note_id = msg.debit_note_id;
        let acceptance = msg.acceptance;
        let node_id = msg.issuer_id;

        log::debug!(
            "Got AcceptDebitNote [{}] from Node [{}].",
            debit_note_id,
            sender_id
        );
        counter!("payment.debit_notes.provider.accepted.call", 1);

        let dao: DebitNoteDao = db.as_dao();
        let debit_note: DebitNote = match dao.get(debit_note_id.clone(), node_id).await {
            Ok(Some(debit_note)) => debit_note,
            Ok(None) => return Err(AcceptRejectError::ObjectNotFound),
            Err(e) => return Err(AcceptRejectError::ServiceError(e.to_string())),
        };

        if sender_id != debit_note.recipient_id.to_string() {
            return Err(AcceptRejectError::Forbidden);
        }

        if debit_note.total_amount_due != acceptance.total_amount_accepted {
            let msg = format!(
                "Invalid amount accepted. Expected: {} Actual: {}",
                debit_note.total_amount_due, acceptance.total_amount_accepted
            );
            return Err(AcceptRejectError::BadRequest(msg));
        }

        match debit_note.status {
            DocumentStatus::Accepted => return Ok(Ack {}),
            DocumentStatus::Settled => return Ok(Ack {}),
            DocumentStatus::Cancelled => {
                return Err(AcceptRejectError::BadRequest(
                    "Cannot accept cancelled debit note".to_owned(),
                ));
            }
            _ => (),
        }

        match dao.accept(debit_note_id.clone(), node_id).await {
            Ok(_) => {
                log::info!("Node [{node_id}] accepted DebitNote [{debit_note_id}].");
                counter!("payment.debit_notes.provider.accepted", 1);
                Ok(Ack {})
            }
            Err(DbError::Query(e)) => Err(AcceptRejectError::BadRequest(e)),
            Err(e) => Err(AcceptRejectError::ServiceError(e.to_string())),
        }
    }

    async fn reject_debit_note(
        db: DbExecutor,
        sender: String,
        msg: RejectDebitNote,
    ) -> Result<Ack, AcceptRejectError> {
        unimplemented!() // TODO
    }

    async fn cancel_debit_note(
        db: DbExecutor,
        sender: String,
        msg: CancelDebitNote,
    ) -> Result<Ack, CancelError> {
        unimplemented!() // TODO
    }

    // *************************** INVOICE ****************************

    async fn send_invoice(
        db: DbExecutor,
        sender_id: String,
        msg: SendInvoice,
    ) -> Result<Ack, SendError> {
        let invoice = msg.0;
        let invoice_id = invoice.invoice_id.clone();
        let agreement_id = invoice.agreement_id.clone();
        let activity_ids = invoice.activity_ids.clone();

        log::debug!(
            "Got SendInvoice [{}] from Node [{}].",
            invoice_id,
            sender_id
        );
        counter!("payment.invoices.requestor.received.call", 1);

        let agreement = match get_agreement(
            agreement_id.clone(),
            ya_client_model::market::Role::Requestor,
        )
        .await
        {
            Err(e) => {
                return Err(SendError::ServiceError(e.to_string()));
            }
            Ok(None) => {
                return Err(SendError::BadRequest(format!(
                    "Agreement {} not found",
                    invoice.agreement_id
                )));
            }
            Ok(Some(agreement)) => agreement,
        };

        for activity_id in activity_ids.iter() {
            match provider::get_agreement_id(
                activity_id.clone(),
                ya_client_model::market::Role::Requestor,
            )
            .await
            {
                Ok(Some(id)) if id != agreement_id => {
                    return Err(SendError::BadRequest(format!(
                        "Activity {} belongs to agreement {} not {}",
                        activity_id, id, agreement_id
                    )));
                }
                Ok(None) => {
                    return Err(SendError::BadRequest(format!(
                        "Activity not found: {}",
                        activity_id
                    )));
                }
                Err(e) => return Err(SendError::ServiceError(e.to_string())),
                _ => (),
            }
        }

        let offeror_id = agreement.offer.provider_id.to_string();
        let issuer_id = invoice.issuer_id.to_string();
        if sender_id != offeror_id || sender_id != issuer_id {
            return Err(SendError::BadRequest("Invalid sender node ID".to_owned()));
        }

        let owner_id = *agreement.requestor_id();
        let sender_id = *agreement.provider_id();
        match async move {
            db.as_dao::<AgreementDao>()
                .create_if_not_exists(agreement, owner_id, Role::Requestor)
                .await?;

            let dao: ActivityDao = db.as_dao();
            for activity_id in activity_ids {
                dao.create_if_not_exists(
                    activity_id,
                    owner_id,
                    Role::Requestor,
                    agreement_id.clone(),
                )
                .await?;
            }

            db.as_dao::<InvoiceDao>().insert_received(invoice).await?;

            log::info!(
                "Invoice [{invoice_id}] for Agreement [{agreement_id}] received from node [{sender_id}]."
            );
            counter!("payment.invoices.requestor.received", 1);
            Ok(())
        }
        .await
        {
            Ok(_) => Ok(Ack {}),
            Err(DbError::Query(e)) => Err(SendError::BadRequest(e)),
            Err(e) => Err(SendError::ServiceError(e.to_string())),
        }
    }

    async fn accept_invoice(
        db: DbExecutor,
        sender_id: String,
        msg: AcceptInvoice,
    ) -> Result<Ack, AcceptRejectError> {
        let invoice_id = msg.invoice_id;
        let acceptance = msg.acceptance;
        let node_id = msg.issuer_id;

        log::debug!(
            "Got AcceptInvoice [{}] from Node [{}].",
            invoice_id,
            sender_id
        );
        counter!("payment.invoices.provider.accepted.call", 1);

        let dao: InvoiceDao = db.as_dao();
        let invoice: Invoice = match dao.get(invoice_id.clone(), node_id).await {
            Ok(Some(invoice)) => invoice,
            Ok(None) => return Err(AcceptRejectError::ObjectNotFound),
            Err(e) => return Err(AcceptRejectError::ServiceError(e.to_string())),
        };

        if sender_id != invoice.recipient_id.to_string() {
            return Err(AcceptRejectError::Forbidden);
        }

        if invoice.amount != acceptance.total_amount_accepted {
            let msg = format!(
                "Invalid amount accepted. Expected: {} Actual: {}",
                invoice.amount, acceptance.total_amount_accepted
            );
            return Err(AcceptRejectError::BadRequest(msg));
        }

        match invoice.status {
            DocumentStatus::Accepted => return Ok(Ack {}),
            DocumentStatus::Settled => return Ok(Ack {}),
            DocumentStatus::Cancelled => {
                return Err(AcceptRejectError::BadRequest(
                    "Cannot accept cancelled invoice".to_owned(),
                ));
            }
            _ => (),
        }

        match dao.accept(invoice_id.clone(), node_id).await {
            Ok(_) => {
                log::info!(
                    "Node [{}] accepted invoice [{}] for Agreement [{}].",
                    node_id,
                    invoice_id,
                    invoice.agreement_id
                );
                counter!("payment.invoices.provider.accepted", 1);
                Ok(Ack {})
            }
            Err(DbError::Query(e)) => Err(AcceptRejectError::BadRequest(e)),
            Err(e) => Err(AcceptRejectError::ServiceError(e.to_string())),
        }
    }

    async fn reject_invoice(
        db: DbExecutor,
        sender: String,
        msg: RejectInvoice,
    ) -> Result<Ack, AcceptRejectError> {
        unimplemented!() // TODO
    }

    async fn cancel_invoice(
        db: DbExecutor,
        sender_id: String,
        msg: CancelInvoice,
    ) -> Result<Ack, CancelError> {
        let invoice_id = msg.invoice_id;

        log::debug!(
            "Got CancelInvoice [{}] from Node [{}].",
            invoice_id,
            sender_id
        );
        counter!("payment.invoices.requestor.cancelled.call", 1);

        let dao: InvoiceDao = db.as_dao();
        let invoice: Invoice = match dao.get(invoice_id.clone(), msg.recipient_id).await {
            Ok(Some(invoice)) => invoice,
            Ok(None) => return Err(CancelError::ObjectNotFound),
            Err(e) => return Err(CancelError::ServiceError(e.to_string())),
        };

        if sender_id != invoice.issuer_id.to_string() {
            return Err(CancelError::Forbidden);
        }

        match invoice.status {
            DocumentStatus::Issued => (),
            DocumentStatus::Received => (),
            DocumentStatus::Rejected => (),
            DocumentStatus::Cancelled => return Ok(Ack {}),
            DocumentStatus::Accepted | DocumentStatus::Settled | DocumentStatus::Failed => {
                return Err(CancelError::Conflict);
            }
        }

        match dao.cancel(invoice_id.clone(), invoice.recipient_id).await {
            Ok(_) => {
                log::info!(
                    "Node [{}] cancelled invoice [{}] for Agreement [{}]..",
                    invoice.recipient_id,
                    invoice_id,
                    invoice.agreement_id
                );
                counter!("payment.invoices.requestor.cancelled", 1);
                Ok(Ack {})
            }
            Err(e) => Err(CancelError::ServiceError(e.to_string())),
        }
    }

    // *************************** PAYMENT ****************************

    async fn send_payment(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender_id: String,
        msg: SendPayment,
    ) -> Result<Ack, SendError> {
        let payment = msg.payment;
        let signature = msg.signature;
        if sender_id != payment.payer_id.to_string() {
            return Err(SendError::BadRequest("Invalid payer ID".to_owned()));
        }

        let platform = payment.payment_platform.clone();
        let amount = payment.amount.clone();
        let num_paid_invoices = payment.agreement_payments.len() as u64;
        match processor
            .lock()
            .await
            .verify_payment(payment, signature)
            .await
        {
            Ok(_) | Err(_) => {
                counter!("payment.amount.received", ya_metrics::utils::cryptocurrency_to_u64(&amount), "platform" => platform);
                counter!("payment.invoices.provider.paid", num_paid_invoices);
                Ok(Ack {})
            }
            // Err(e) => match e {
            //    VerifyPaymentError::ConfirmationEncoding => {
            //        Err(SendError::BadRequest(e.to_string()))
            //    }
            //    VerifyPaymentError::Validation(e) => Err(SendError::BadRequest(e)),
            //    _ => Err(SendError::ServiceError(e.to_string())),
            //},
        }
    }

    // **************************** SYNC *****************************
    async fn sync_payment(
        db: DbExecutor,
        processor: Arc<Mutex<PaymentProcessor>>,
        sender_id: String,
        msg: PaymentSync,
    ) -> Result<Ack, PaymentSyncError> {
        let mut errors = PaymentSyncError::default();

        for payment_send in msg.payments {
            let result = send_payment(
                db.clone(),
                Arc::clone(&processor),
                sender_id.clone(),
                payment_send,
            )
            .await;

            if let Err(e) = result {
                errors.payment_send_errors.push(e);
            }
        }

        for invoice_accept in msg.invoice_accepts {
            let result = accept_invoice(db.clone(), sender_id.clone(), invoice_accept).await;
            if let Err(e) = result {
                errors.accept_errors.push(e);
            }
        }

        for debit_note_accept in msg.debit_note_accepts {
            let result = accept_debit_note(db.clone(), sender_id.clone(), debit_note_accept).await;
            if let Err(e) = result {
                errors.accept_errors.push(e);
            }
        }

        if errors.accept_errors.is_empty() && errors.payment_send_errors.is_empty() {
            Ok(Ack {})
        } else {
            Err(errors)
        }
    }
}
