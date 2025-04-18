use crate::dao::{DebitNoteDao, InvoiceDao};
use crate::processor::PaymentProcessor;
use crate::Config;

use futures::prelude::*;
use metrics::counter;
use std::collections::HashMap;
use std::sync::Arc;

use ya_core_model::payment::local::{GenericError, BUS_ID as PAYMENT_BUS_ID};
use ya_core_model::payment::public::{AcceptDebitNote, AcceptInvoice, PaymentSync, SendPayment};

use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::{service, ServiceBinder};

pub async fn bind_service(
    db: &DbExecutor,
    processor: Arc<PaymentProcessor>,
    config: Arc<Config>,
) -> anyhow::Result<()> {
    log::debug!("Binding payment service to service bus");

    local::bind_service(db, processor.clone()).await?;
    public::bind_service(db, processor, config).await?;

    log::debug!("Successfully bound payment service to service bus");
    Ok(())
}

mod local {
    use super::*;
    use crate::dao::*;
    use chrono::DateTime;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    use std::{collections::BTreeMap, convert::TryInto};
    use tracing::{debug, trace};
    use ya_core_model::identity;
    use ya_service_bus::RpcEndpoint;

    use ya_client_model::{
        payment::{
            Account, DebitNoteEventType, DocumentStatus, DriverDetails, DriverStatusProperty,
            InvoiceEventType,
        },
        NodeId,
    };
    use ya_core_model::driver::ValidateAllocationResult;
    use ya_core_model::identity::event::IdentityEvent;
    use ya_core_model::payment::public::Ack;
    use ya_core_model::{
        driver::{driver_bus_id, DriverStatus, DriverStatusError},
        payment::local::*,
    };
    use ya_persistence::types::Role;

    pub async fn bind_service(
        db: &DbExecutor,
        processor: Arc<PaymentProcessor>,
    ) -> anyhow::Result<()> {
        log::debug!("Binding payment local service to service bus");

        ServiceBinder::new(BUS_ID, db, processor)
            .bind_with_processor(register_driver)
            .bind_with_processor(unregister_driver)
            .bind_with_processor(register_account)
            .bind_with_processor(account_event)
            .bind_with_processor(unregister_account)
            .bind_with_processor(notify_payment)
            .bind_with_processor(get_rpc_endpoints)
            .bind_with_processor(get_status)
            .bind_with_processor(get_invoice_stats)
            .bind_with_processor(get_accounts)
            .bind_with_processor(validate_allocation)
            .bind_with_processor(release_allocations)
            .bind_with_processor(process_payments_now)
            .bind_with_processor(process_cycle_info)
            .bind_with_processor(process_cycle_set)
            .bind_with_processor(get_drivers)
            .bind_with_processor(payment_driver_status)
            .bind_with_processor(handle_status_change)
            .bind_with_processor(shut_down);

        log::debug!("Subscribing to identity events...");
        service(identity::BUS_ID)
            .send(identity::Subscribe {
                endpoint: BUS_ID.to_string(),
            })
            .await??;
        log::debug!("Successfully subscribed payment module service to identity events.");

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

        counter!("payment.debit_notes.events.query", 0);
        counter!("payment.invoices.events.query", 0);

        counter!("payment.invoices.provider.issued", 0);
        counter!("payment.invoices.provider.sent", 0);
        counter!("payment.invoices.provider.sent.call", 0);
        counter!("payment.invoices.provider.cancelled", 0);
        counter!("payment.invoices.provider.cancelled.call", 0);
        counter!("payment.invoices.provider.paid", 0);
        counter!("payment.invoices.provider.accepted", 0);
        counter!("payment.invoices.provider.accepted.call", 0);
        counter!("payment.invoices.requestor.not-enough-funds", 0);

        counter!("payment.amount.received", 0, "platform" => "erc20-holesky-tglm");
        counter!("payment.amount.received", 0, "platform" => "erc20-mainnet-glm");
        counter!("payment.amount.received", 0, "platform" => "erc20-polygon-glm");

        counter!("payment.amount.sent", 0, "platform" => "erc20-holesky-tglm");
        counter!("payment.amount.sent", 0, "platform" => "erc20-mainnet-glm");
        counter!("payment.amount.sent", 0, "platform" => "erc20-polygon-glm");

        log::debug!("Successfully bound payment local service to service bus");
        Ok(())
    }

    async fn register_driver(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: RegisterDriver,
    ) -> Result<(), RegisterDriverError> {
        let driver = msg.driver_name.clone();
        debug!(
            entity = "driver",
            action = "register",
            driver,
            "Register driver started"
        );
        let res = processor.register_driver(msg).await;
        trace!(
            entity = "driver",
            action = "register",
            driver,
            "Register driver finished"
        );
        res
    }

    async fn unregister_driver(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: UnregisterDriver,
    ) -> Result<(), UnregisterDriverError> {
        let driver = msg.0.clone();
        debug!(
            entity = "driver",
            action = "unregister",
            driver,
            "Unregister driver started"
        );
        let res = processor.unregister_driver(msg).await;
        trace!(
            entity = "driver",
            action = "unregister",
            driver,
            "Unregister driver finished"
        );
        res
    }
    async fn account_event(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: IdentityEvent,
    ) -> Result<(), ya_core_model::identity::Error> {
        debug!(
            entity = "account",
            action = "event",
            "Payment service account event handling"
        );
        processor.identity_event(msg).await.map_err(|e| {
            ya_core_model::identity::Error::InternalErr(format!(
                "Payment processor - Failed to process account event: {}",
                e
            ))
        })
    }
    async fn register_account(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: RegisterAccount,
    ) -> Result<(), RegisterAccountError> {
        let platform = format!("{}-{}-{}", &msg.driver, &msg.network, &msg.token);
        let account = msg.address.clone();
        debug!(
            entity = "account",
            action = "register",
            account,
            platform,
            "Register account started"
        );
        let res = processor.register_account(msg).await;
        trace!(
            entity = "account",
            action = "register",
            account,
            platform,
            "Register account finished"
        );
        res
    }

    async fn unregister_account(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: UnregisterAccount,
    ) -> Result<(), UnregisterAccountError> {
        let account = msg.address.clone();
        debug!(
            entity = "account",
            action = "unregister",
            account,
            "Unregister account started"
        );
        processor.unregister_account(msg).await?;
        trace!(
            entity = "account",
            action = "unregister",
            account,
            "Unregister account finished"
        );
        Ok(())
    }

    async fn get_accounts(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: GetAccounts,
    ) -> Result<Vec<Account>, GetAccountsError> {
        trace!(entity = "accounts", action = "get", "Get accounts started");
        let res = processor.get_accounts().await;
        trace!(entity = "accounts", action = "get", "Get accounts finished");
        res
    }

    async fn notify_payment(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: NotifyPayment,
    ) -> Result<(), GenericError> {
        static NOTIFY_COUNTER: AtomicUsize = AtomicUsize::new(0);
        let i = NOTIFY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let start = Instant::now();

        debug!(
            entity = "payment",
            action = "notify",
            sender = msg.sender,
            recipient = msg.recipient,
            no = i,
            "Notify payment started"
        );
        let res = processor.notify_payment(msg.clone()).await;

        debug!(
            entity = "payment",
            action = "notify",
            sender = msg.sender,
            recipient = msg.recipient,
            no = i,
            duration = format!(
                "Notify payment finished after {:.2}s",
                start.elapsed().as_secs_f32()
            )
        );

        Ok(res?)
    }

    async fn get_rpc_endpoints(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: GetRpcEndpoints,
    ) -> Result<GetRpcEndpointsResult, GenericError> {
        let GetRpcEndpoints {
            driver,
            network,
            address,
            verify,
            resolve,
            no_wait,
        } = msg;

        let (network2, network_details) = processor
            .get_network(driver.to_string(), network.as_ref().map(|s| s.to_string()))
            .await
            .map_err(GenericError::new)?;
        let network2 = NetworkName::from_str(&network2).map_err(GenericError::new)?;

        let token = network_details.default_token.clone();
        let platform = match network_details.tokens.get(&token) {
            Some(platform) => platform.clone(),
            None => {
                return Err(GenericError::new(format!(
                    "Unsupported token. driver={} network={} token={}",
                    driver, network2, token
                )));
            }
        };

        let rpc_info = processor
            .get_rpc_endpoints_info(
                platform,
                address.to_string(),
                network.as_ref().map(|s| s.to_string()),
                verify,
                resolve,
                no_wait,
            )
            .await
            .map_err(GenericError::new)?;

        Ok(GetRpcEndpointsResult {
            endpoints: rpc_info.endpoints,
            sources: rpc_info.sources,
        })
    }

    async fn get_status(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: GetStatus,
    ) -> Result<StatusResult, GenericError> {
        let GetStatus {
            address,
            driver,
            network,
            token,
            after_timestamp,
        } = msg;

        let (network, network_details) = processor
            .get_network(driver.clone(), network)
            .await
            .map_err(GenericError::new)?;
        let token = token.unwrap_or_else(|| network_details.default_token.clone());
        let after_timestamp = DateTime::from_timestamp(after_timestamp, 0)
            .expect("Failed on out-of-range number of seconds")
            .naive_utc();
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
                .get_status(platform.clone(), address.clone())
                .await
        }
        .map_err(GenericError::new);

        let (incoming, outgoing, status, reserved) =
            future::try_join4(incoming_fut, outgoing_fut, amount_fut, reserved_fut).await?;

        Ok(StatusResult {
            amount: status.token_balance,
            reserved,
            outgoing,
            incoming,
            driver,
            network,
            token,
            gas: status.gas_details,
            block_number: 0,
            block_datetime: Default::default(),
        })
    }

    async fn get_invoice_stats(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
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
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: ValidateAllocation,
    ) -> Result<ValidateAllocationResult, ValidateAllocationError> {
        Ok(processor
            .validate_allocation(
                msg.platform,
                msg.address,
                msg.amount,
                msg.timeout,
                msg.deposit,
                msg.new_allocation,
            )
            .await?)
    }

    async fn release_allocations(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: ReleaseAllocations,
    ) -> Result<(), GenericError> {
        log::debug!("Release allocations processor started");
        processor.release_allocations(true).await;
        log::debug!("Release allocations processor finished");
        Ok(())
    }

    async fn process_cycle_info(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: ProcessBatchCycleInfo,
    ) -> Result<ProcessBatchCycleResponse, ProcessBatchCycleError> {
        processor.process_cycle_info(msg).await
    }

    async fn process_cycle_set(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: ProcessBatchCycleSet,
    ) -> Result<ProcessBatchCycleResponse, ProcessBatchCycleError> {
        processor.process_cycle_set(msg).await
    }

    async fn process_payments_now(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: ProcessPaymentsNow,
    ) -> Result<ProcessPaymentsNowResponse, ProcessPaymentsError> {
        processor.process_payments_now(msg).await
    }

    async fn get_drivers(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: GetDrivers,
    ) -> Result<HashMap<String, DriverDetails>, GetDriversError> {
        processor.get_drivers(msg.ignore_legacy_networks).await
    }

    async fn payment_driver_status(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: PaymentDriverStatus,
    ) -> Result<Vec<DriverStatusProperty>, PaymentDriverStatusError> {
        let drivers = match &msg.driver {
            Some(driver) => vec![driver.clone()],
            None => {
                #[allow(clippy::iter_kv_map)]
                // Unwrap is provably safe because NoError can't be instanciated
                match service(PAYMENT_BUS_ID)
                    .call(GetDrivers {
                        ignore_legacy_networks: false,
                    })
                    .await
                {
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

    // *************************** PAYMENT ****************************
    async fn handle_status_change(
        db: DbExecutor,
        _processor: Arc<PaymentProcessor>,
        _caller: String,
        msg: PaymentDriverStatusChange,
    ) -> Result<Ack, GenericError> {
        /// Payment platform affected by status
        ///
        /// It doesn't contain the token because we don't actually
        /// support multiple tokens on one chain.
        ///
        /// TODO: remove references to token stuff in yagna and ideally
        /// make payment platforms properly typed along the way.
        #[derive(Hash, PartialEq, Eq)]
        struct Platform {
            driver: String,
            network: String,
        }

        impl Platform {
            fn new(driver: impl Into<String>, network: impl Into<String>) -> Self {
                Platform {
                    driver: driver.into(),
                    network: network.into(),
                }
            }
        }

        let platform_str_to_platform = |platform: &str| -> Result<Platform, GenericError> {
            let parts = platform.split('-').collect::<Vec<_>>();
            let [driver, network, _]: [_; 3] = parts.try_into().map_err(|_| {
                GenericError::new("Payment platform must be of the form {driver}-{network}-{token}")
            })?;

            Ok(Platform::new(driver, network))
        };

        /// Event broadcast information
        ///
        /// Each status property shall be broadcasted to all debit notes
        /// and invoices affected.
        ///
        /// If properties are empty, a PaymentOkEvent will be sent.
        #[derive(Default)]
        struct Broadcast {
            debit_notes: Vec<(String, NodeId)>,
            invoices: Vec<(String, NodeId)>,
            properties: Vec<DriverStatusProperty>,
        }

        // Create a mapping between platforms and relevant properties.
        //
        // This relies on the fact that a given payment driver status property
        // can only affect one platform.
        let mut broadcast = HashMap::<Platform, Broadcast>::default();
        for prop in msg.properties {
            let Some(network) = prop.network() else {
                continue;
            };

            let value = broadcast
                .entry(Platform::new(prop.driver(), network))
                .or_default();
            value.properties.push(prop);
        }

        // All DAOs
        let debit_note_dao: DebitNoteDao = db.as_dao();
        let debit_note_ev_dao: DebitNoteEventDao = db.as_dao();
        let invoice_dao: InvoiceDao = db.as_dao();
        let invoice_ev_dao: InvoiceEventDao = db.as_dao();

        let accepted_notes = debit_note_dao
            .list(
                Some(Role::Requestor),
                Some(DocumentStatus::Accepted),
                Some(true),
                None,
            )
            .await
            .map_err(GenericError::new)?;

        // Populate broadcasts with affected debit_notes
        for debit_note in accepted_notes {
            let platform = platform_str_to_platform(&debit_note.payment_platform)?;

            // checks if the last payment-status event was PAYMENT_OK or no such event was emitted
            let was_already_ok = debit_note_ev_dao
                .get_for_debit_note_id(
                    debit_note.debit_note_id.clone(),
                    None,
                    None,
                    None,
                    vec!["PAYMENT_EVENT".into(), "PAYMENT_OK".into()],
                    vec![],
                )
                .await
                .map_err(GenericError::new)?
                .last()
                .map(|ev_type| {
                    matches!(
                        &ev_type.event_type,
                        DebitNoteEventType::DebitNotePaymentOkEvent
                    )
                })
                .unwrap_or(true);

            if !was_already_ok {
                // If debit note has reported driver errors before, we *must* send a broadcast on status change.
                // This will either be a new problem, or PaymentOkEvent if no errors are found.
                broadcast
                    .entry(platform)
                    .or_default()
                    .debit_notes
                    .push((debit_note.debit_note_id, debit_note.issuer_id));
            } else if let Some(broadcast) = broadcast.get_mut(&platform) {
                broadcast
                    .debit_notes
                    .push((debit_note.debit_note_id, debit_note.issuer_id));
            }
        }

        let accepted_invoices = invoice_dao
            .list(Some(Role::Requestor), Some(DocumentStatus::Accepted))
            .await
            .map_err(GenericError::new)?;

        // Populate broadcasts with affected invoices
        for invoice in accepted_invoices {
            let platform = platform_str_to_platform(&invoice.payment_platform)?;

            // checks if the last payment-status event was PAYMENT_OK or no such event was emitted
            let was_already_ok = invoice_ev_dao
                .get_for_invoice_id(
                    invoice.invoice_id.clone(),
                    None,
                    None,
                    None,
                    vec!["PAYMENT_EVENT".into(), "PAYMENT_OK".into()],
                    vec![],
                )
                .await
                .map_err(GenericError::new)?
                .last()
                .map(|ev_type| {
                    matches!(&ev_type.event_type, InvoiceEventType::InvoicePaymentOkEvent)
                })
                .unwrap_or(true);

            if !was_already_ok {
                // If invoice has reported driver errors before, we *must* send a broadcast on status change.
                // This will either be a new problem, or PaymentOkEvent if no errors are found.
                broadcast
                    .entry(platform)
                    .or_default()
                    .invoices
                    .push((invoice.invoice_id, invoice.issuer_id));
            } else if let Some(broadcast) = broadcast.get_mut(&platform) {
                broadcast
                    .invoices
                    .push((invoice.invoice_id, invoice.issuer_id));
            }
        }

        // Emit debit note & invoice events.
        for broadcast in broadcast.into_values() {
            // If properties are empty, send OkEvents. Otherwise send the wrapped properties.
            if broadcast.properties.is_empty() {
                for (debit_note_id, owner_id) in &broadcast.debit_notes {
                    debit_note_ev_dao
                        .create(
                            debit_note_id.clone(),
                            *owner_id,
                            DebitNoteEventType::DebitNotePaymentOkEvent,
                        )
                        .await
                        .map_err(GenericError::new)?;
                }

                for (invoice_id, owner_id) in &broadcast.invoices {
                    invoice_ev_dao
                        .create(
                            invoice_id.clone(),
                            *owner_id,
                            InvoiceEventType::InvoicePaymentOkEvent,
                        )
                        .await
                        .map_err(GenericError::new)?;
                }
            } else {
                for prop in broadcast.properties {
                    for (invoice_id, owner_id) in &broadcast.invoices {
                        invoice_ev_dao
                            .create(
                                invoice_id.clone(),
                                *owner_id,
                                InvoiceEventType::InvoicePaymentStatusEvent {
                                    property: prop.clone(),
                                },
                            )
                            .await
                            .map_err(GenericError::new)?;
                    }
                    for (debit_note_id, owner_id) in &broadcast.debit_notes {
                        debit_note_ev_dao
                            .create(
                                debit_note_id.clone(),
                                *owner_id,
                                DebitNoteEventType::DebitNotePaymentStatusEvent {
                                    property: prop.clone(),
                                },
                            )
                            .await
                            .map_err(GenericError::new)?;
                    }
                }
            }
        }

        Ok(Ack {})
    }

    async fn shut_down(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender: String,
        msg: ShutDown,
    ) -> Result<(), GenericError> {
        // It's crucial to drop the lock on processor (hence assigning the future to a variable).
        // Otherwise, we won't be able to handle calls to `notify_payment` sent by drivers during shutdown.
        let shutdown_future = processor.shut_down(msg.timeout).await;
        shutdown_future.await;
        Ok(())
    }
}

mod public {
    use std::str::FromStr;
    use tracing::debug;

    use super::*;

    use crate::error::processor::VerifyPaymentError;
    use crate::error::DbError;
    use crate::payment_sync::{send_sync_notifs_job, send_sync_requests};
    use crate::utils::*;
    use crate::{dao::*, payment_sync::SYNC_NOTIFS_NOTIFY};

    // use crate::error::processor::VerifyPaymentError;
    use ya_client_model::{payment::*, NodeId};
    use ya_core_model::payment::public::*;
    use ya_persistence::types::Role;
    use ya_std_utils::LogErr;

    pub async fn bind_service(
        db: &DbExecutor,
        processor: Arc<PaymentProcessor>,
        config: Arc<Config>,
    ) -> anyhow::Result<()> {
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
            .bind(sync_request)
            .bind_with_processor(send_payment)
            .bind_with_processor(send_payment_with_bytes)
            .bind_with_processor(sync_payment)
            .bind_with_processor(sync_payment_with_bytes);

        if config.sync_notif_backoff.run_sync_job {
            send_sync_notifs_job(db.clone(), config);
            send_sync_requests(db.clone());
        }

        log::debug!("Successfully bound payment public service to service bus");
        Ok(())
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
        let debit_note: DebitNote = match dao.get(debit_note_id.clone(), Some(node_id)).await {
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
                log::info!("Node [{sender_id}] accepted DebitNote [{debit_note_id}].");
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
        let owner_id = msg.issuer_id;

        log::debug!(
            "Got AcceptInvoice [{}] from Node [{}].",
            invoice_id,
            sender_id
        );
        counter!("payment.invoices.provider.accepted.call", 1);

        let dao: InvoiceDao = db.as_dao();
        let invoice: Invoice = match dao.get(invoice_id.clone(), owner_id).await {
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

        match dao.accept(invoice_id.clone(), owner_id).await {
            Ok(_) => {
                log::info!(
                    "Node [{}] accepted invoice [{}] for Agreement [{}].",
                    sender_id,
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
        sender_id: String,
        msg: RejectInvoiceV2,
    ) -> Result<Ack, AcceptRejectError> {
        let invoice_id = msg.invoice_id;
        let rejection = msg.rejection;
        let owner_id = msg.issuer_id;

        log::debug!(
            "Got RejectInvoiceV2 [{}] from Node [{}].",
            invoice_id,
            sender_id,
        );
        counter!("payment.invoices.provider.rejected.call", 1);

        let dao: InvoiceDao = db.as_dao();
        let invoice: Invoice = match dao.get(invoice_id.clone(), owner_id).await {
            Ok(Some(invoice)) => invoice,
            Ok(None) => return Err(AcceptRejectError::ObjectNotFound),
            Err(e) => return Err(AcceptRejectError::ServiceError(e.to_string())),
        };

        if sender_id != invoice.recipient_id.to_string() {
            return Err(AcceptRejectError::Forbidden);
        }

        match invoice.status {
            status @ DocumentStatus::Accepted
            | status @ DocumentStatus::Settled
            | status @ DocumentStatus::Cancelled => {
                return Err(AcceptRejectError::BadRequest(format!(
                    "Cannot reject {status:?} invoice"
                )));
            }
            DocumentStatus::Rejected => return Ok(Ack {}),
            _ => (),
        }

        match dao.reject(invoice_id.clone(), owner_id, rejection).await {
            Ok(_) => {
                log::info!(
                    "Node [{}] rejected invoice [{}] for Agreement [{}].",
                    owner_id,
                    invoice_id,
                    invoice.agreement_id
                );
                counter!("payment.invoices.provider.rejected", 1);
                Ok(Ack {})
            }
            Err(DbError::Query(e)) => Err(AcceptRejectError::BadRequest(e)),
            Err(e) => Err(AcceptRejectError::ServiceError(e.to_string())),
        }
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

    async fn send_payment(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender_id: String,
        msg: SendPayment,
    ) -> Result<Ack, SendError> {
        send_payment_impl(db, processor, sender_id, msg.payment, msg.signature, None).await
    }

    async fn send_payment_with_bytes(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender_id: String,
        msg: SendSignedPayment,
    ) -> Result<Ack, SendError> {
        send_payment_impl(
            db,
            processor,
            sender_id,
            msg.payment,
            msg.signature,
            Some(msg.signed_bytes),
        )
        .await
    }

    async fn send_payment_impl(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender_id: String,
        payment: Payment,
        signature: Vec<u8>,
        canonical: Option<Vec<u8>>,
    ) -> Result<Ack, SendError> {
        let payment_id = payment.payment_id.clone();
        if sender_id != payment.payer_id.to_string() {
            return Err(SendError::BadRequest("Invalid payer ID".to_owned()));
        }

        let platform = payment.payment_platform.clone();
        let amount = payment.amount.clone();
        let num_paid_invoices = payment.agreement_payments.len() as u64;

        debug!(
            entity = "payment",
            action = "verify",
            payment_id,
            "Verify payment processor started."
        );
        let res = match processor
            .verify_payment(payment, signature, canonical)
            .await
        {
            Ok(_) => {
                counter!("payment.amount.received", ya_metrics::utils::cryptocurrency_to_u64(&amount), "platform" => platform);
                counter!("payment.invoices.provider.paid", num_paid_invoices);
                Ok(Ack {})
            }
            Err(e) => match e {
                VerifyPaymentError::ConfirmationEncoding => {
                    Err(SendError::BadRequest(e.to_string()))
                }
                VerifyPaymentError::Validation(e) => Err(SendError::BadRequest(e)),
                _ => Err(SendError::ServiceError(e.to_string())),
            },
        }.log_err_msg("Payment verification failure");

        debug!(
            entity = "payment",
            action = "verify",
            payment_id,
            "Verify payment processor finished."
        );
        res
    }

    // **************************** SYNC *****************************
    async fn sync_request(
        db: DbExecutor,
        sender_id: String,
        msg: PaymentSyncRequest,
    ) -> Result<Ack, SendError> {
        let dao: SyncNotifsDao = db.as_dao();

        let peer_id =
            NodeId::from_str(&sender_id).expect("sender_id supplied by ya_service_bus is invalid");
        dao.upsert(peer_id)
            .await
            .map_err(|e| SendError::BadRequest(e.to_string()))?;
        SYNC_NOTIFS_NOTIFY.notify_one();

        Ok(Ack {})
    }

    async fn sync_payment(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender_id: String,
        msg: PaymentSync,
    ) -> Result<Ack, PaymentSyncError> {
        sync_payment_impl(
            db,
            processor,
            sender_id,
            msg.payments,
            send_payment,
            msg.invoice_accepts,
            msg.invoice_rejects,
            msg.debit_note_accepts,
        )
        .await
    }

    async fn sync_payment_with_bytes(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender_id: String,
        msg: PaymentSyncWithBytes,
    ) -> Result<Ack, PaymentSyncError> {
        sync_payment_impl(
            db,
            processor,
            sender_id,
            msg.payments,
            send_payment_with_bytes,
            msg.invoice_accepts,
            msg.invoice_rejects,
            msg.debit_note_accepts,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn sync_payment_impl<PaymentType, PaymentProcessorFunc, Fut>(
        db: DbExecutor,
        processor: Arc<PaymentProcessor>,
        sender_id: String,
        payments: Vec<PaymentType>,
        payment_processor_func: PaymentProcessorFunc,
        invoice_accepts: Vec<AcceptInvoice>,
        invoice_rejects: Vec<RejectInvoiceV2>,
        debit_note_accepts: Vec<AcceptDebitNote>,
    ) -> Result<Ack, PaymentSyncError>
    where
        PaymentProcessorFunc: Fn(DbExecutor, Arc<PaymentProcessor>, String, PaymentType) -> Fut,
        Fut: Future<Output = Result<Ack, SendError>>,
    {
        let mut errors = PaymentSyncError::default();

        for payment_send in payments {
            let result = payment_processor_func(
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

        for invoice_accept in invoice_accepts {
            let result = accept_invoice(db.clone(), sender_id.clone(), invoice_accept).await;
            if let Err(e) = result {
                errors.accept_errors.push(e);
            }
        }

        for invoice_reject in invoice_rejects {
            let result = reject_invoice(db.clone(), sender_id.clone(), invoice_reject).await;
            if let Err(e) = result {
                errors.accept_errors.push(e);
            }
        }

        for debit_note_accept in debit_note_accepts {
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
