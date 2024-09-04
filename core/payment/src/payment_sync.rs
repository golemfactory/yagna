use std::sync::Arc;
use std::{collections::HashSet, time::Duration};

use crate::utils::remove_allocation_ids_from_payment;
use crate::Config;

use chrono::{DateTime, Utc};
use tokio::sync::Notify;
use ya_client_model::{
    payment::{Acceptance, InvoiceEventType},
    NodeId,
};
use ya_core_model::driver::SignPaymentCanonicalized;
use ya_core_model::{
    driver::{driver_bus_id, SignPayment},
    identity::{self, IdentityInfo},
    payment::{
        self,
        local::GenericError,
        public::{
            AcceptDebitNote, AcceptInvoice, PaymentSync, PaymentSyncRequest, PaymentSyncWithBytes,
            RejectInvoiceV2, SendPayment, SendSignedPayment,
        },
    },
};
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{timeout::IntoTimeoutFuture, typed, RpcEndpoint};

use crate::dao::{DebitNoteDao, InvoiceDao, InvoiceEventDao, PaymentDao, SyncNotifsDao};

const REMOTE_CALL_TIMEOUT: Duration = Duration::from_secs(30);

async fn payment_sync(
    db: &DbExecutor,
    owner: NodeId,
    peer_id: NodeId,
) -> anyhow::Result<(PaymentSync, PaymentSyncWithBytes)> {
    let payment_dao: PaymentDao = db.as_dao();
    let invoice_dao: InvoiceDao = db.as_dao();
    let debit_note_dao: DebitNoteDao = db.as_dao();
    let invoice_event_dao: InvoiceEventDao = db.as_dao();

    let mut payments = Vec::default();
    let mut payments_canonicalized = Vec::default();
    for payment in payment_dao.list_unsent(owner, Some(peer_id)).await? {
        let platform_components = payment.payment_platform.split('-').collect::<Vec<_>>();
        let driver = &platform_components[0];
        let bus_id = driver_bus_id(driver);

        let payment = remove_allocation_ids_from_payment(payment);

        let signature = typed::service(bus_id.clone())
            .send(SignPayment(payment.clone()))
            .await??;
        payments.push(SendPayment::new(payment.clone(), signature));

        let signature_canonicalized = typed::service(bus_id.clone())
            .send(SignPaymentCanonicalized(payment.clone()))
            .await??;
        payments_canonicalized.push(SendSignedPayment::new(payment, signature_canonicalized));
    }

    let mut invoice_accepts = Vec::default();
    for invoice in invoice_dao.unsent_accepted(owner, peer_id).await? {
        invoice_accepts.push(AcceptInvoice::new(
            invoice.invoice_id,
            Acceptance {
                total_amount_accepted: invoice.amount,
                allocation_id: String::new(),
            },
            peer_id,
        ));
    }

    let mut invoice_rejects = Vec::default();
    for invoice in invoice_dao.unsent_rejected(owner, peer_id).await? {
        let events = invoice_event_dao
            .get_for_invoice_id(
                invoice.invoice_id.clone(),
                None,
                None,
                None,
                vec!["REJECTED".into()],
                vec![],
            )
            .await
            .map_err(GenericError::new)?;
        if let Some(event) = events.into_iter().last() {
            if let InvoiceEventType::InvoiceRejectedEvent { rejection } = event.event_type {
                invoice_rejects.push(RejectInvoiceV2 {
                    invoice_id: invoice.invoice_id,
                    rejection,
                    issuer_id: peer_id,
                });
            };
        };
    }

    let mut debit_note_accepts = Vec::default();
    for debit_note in debit_note_dao.unsent_accepted(owner, peer_id).await? {
        debit_note_accepts.push(AcceptDebitNote::new(
            debit_note.debit_note_id,
            Acceptance {
                total_amount_accepted: debit_note.total_amount_due,
                allocation_id: String::new(),
            },
            peer_id,
        ));
    }

    Ok((
        PaymentSync {
            payments,
            invoice_accepts: invoice_accepts.clone(),
            invoice_rejects: invoice_rejects.clone(),
            debit_note_accepts: debit_note_accepts.clone(),
        },
        PaymentSyncWithBytes {
            payments: payments_canonicalized,
            invoice_accepts,
            invoice_rejects,
            debit_note_accepts,
        },
    ))
}

async fn mark_all_sent(db: &DbExecutor, owner_id: NodeId, msg: PaymentSync) -> anyhow::Result<()> {
    let payment_dao: PaymentDao = db.as_dao();
    let invoice_dao: InvoiceDao = db.as_dao();
    let debit_note_dao: DebitNoteDao = db.as_dao();

    for payment_send in msg.payments {
        log::info!(
            "Delivered Payment confirmation [{}] to [{}]",
            payment_send.payment.payment_id,
            payment_send.payment.payee_id
        );
        payment_dao
            .mark_sent(payment_send.payment.payment_id)
            .await?;
    }

    for invoice_accept in msg.invoice_accepts {
        log::info!(
            "Delivered Invoice [{}] acceptance to [{}]",
            invoice_accept.invoice_id,
            invoice_accept.issuer_id
        );
        invoice_dao
            .mark_accept_sent(invoice_accept.invoice_id, owner_id)
            .await?;
    }

    for invoice_reject in msg.invoice_rejects {
        log::info!(
            "Delivered Invoice [{}] rejection to [{}]",
            invoice_reject.invoice_id,
            invoice_reject.issuer_id
        );
        invoice_dao
            .mark_reject_sent(invoice_reject.invoice_id, owner_id)
            .await?;
    }

    for debit_note_accept in msg.debit_note_accepts {
        log::info!(
            "Delivered DebitNote [{}] acceptance to [{}]",
            debit_note_accept.debit_note_id,
            debit_note_accept.issuer_id
        );
        debit_note_dao
            .mark_accept_sent(debit_note_accept.debit_note_id, owner_id)
            .await?;
    }

    Ok(())
}

async fn send_sync_notifs_for_peer(
    peer: NodeId,
    db: &DbExecutor,
    config: &Config,
    cutoff: &DateTime<Utc>,
) -> anyhow::Result<()> {
    log::debug!("Processing PaymentSync for peer [{peer}].");

    let dao: SyncNotifsDao = db.as_dao();

    let mut all_delivered = true;
    let identities = typed::service(identity::BUS_ID)
        .call(ya_core_model::identity::List {})
        .await??;

    for identity in identities {
        let owner = identity.node_id;
        if identity.is_locked {
            log::info!("Skipping PaymentSync for [{owner}] since identity is locked.");
            continue;
        }

        let (msg, msg_with_bytes) = payment_sync(db, owner, peer).await?;

        log::debug!("Sending PaymentSync as [{owner}] to [{peer}].");
        let mut result = ya_net::from(owner)
            .to(peer)
            .service(ya_core_model::payment::public::BUS_ID)
            .call(msg_with_bytes.clone())
            .await;

        log::debug!("Sending PaymentSync as [{owner}] to [{peer}] result: {result:?}");

        // Centralnet and hybridnet return different errors when the endpoint is not supported, so
        // we have to resort to checking error message.
        // This message will be sent even if the node can handle PaymentSyncWithBytes but is not
        // connected at all, but there is no standard way to differentiate between these cases.
        if matches!(&result, Err(e) if e.to_string().contains("endpoint address not found")) {
            log::debug!("Sending PaymentSync as [{owner}] to [{peer}]: PaymentSyncWithBytes endpoint not found, falling back to PaymentSync.");
            result = ya_net::from(owner)
                .to(peer)
                .service(ya_core_model::payment::public::BUS_ID)
                .call(msg.clone())
                .await;
        }

        if matches!(&result, Ok(Ok(_))) {
            log::debug!("Delivered PaymentSync to [{peer}] as [{owner}].");
            mark_all_sent(db, owner, msg).await?;
        } else {
            all_delivered = false;
            let err = match result {
                Err(x) => x.to_string(),
                Ok(Err(x)) => x.to_string(),
                Ok(Ok(_)) => unreachable!(),
            };
            log::debug!("Couldn't deliver PaymentSync to [{peer}] as [{owner}]: {err}");
            dao.increment_retry(peer, cutoff.naive_utc()).await?;
        }
    }

    if all_delivered {
        dao.drop(peer).await?;
    }

    Ok(())
}

async fn send_sync_notifs(db: &DbExecutor, config: &Config) -> anyhow::Result<Option<Duration>> {
    let cutoff = Utc::now();
    let dao: SyncNotifsDao = db.as_dao();

    let backoff_config = &config.sync_notif_backoff;

    let exp_backoff = |n| {
        let secs = backoff_config.initial_delay * backoff_config.exponent.powi(n) as u32;
        let capped: Duration = if let Some(cap) = backoff_config.cap {
            ::std::cmp::min(cap, secs)
        } else {
            secs
        };
        capped
    };
    let peers_to_notify = dao
        .list()
        .await?
        .into_iter()
        .filter(|entry| {
            let next_deadline = entry.last_ping + exp_backoff(entry.retries as _);
            next_deadline.and_utc() <= cutoff && entry.retries <= backoff_config.max_retries as i32
        })
        .map(|entry| entry.id)
        .collect::<Vec<_>>();

    for peer in peers_to_notify {
        send_sync_notifs_for_peer(peer, db, config, &cutoff).await?;
    }

    // Next sleep duration is calculated after all events were updated to pick up entries
    // that were not delivered in current run.
    let next_sleep_duration = dao
        .list()
        .await?
        .iter()
        .map(|entry| {
            let next_deadline = entry.last_ping + exp_backoff(entry.retries as _);
            next_deadline.and_utc()
        })
        .filter(|deadline| deadline > &cutoff)
        .min()
        .map(|ts| ts - cutoff)
        .and_then(|dur| dur.to_std().ok());

    Ok(next_sleep_duration)
}

lazy_static::lazy_static! {
    pub static ref SYNC_NOTIFS_NOTIFY: Notify = Notify::new();
}

pub fn send_sync_notifs_job(db: DbExecutor, config: Arc<Config>) {
    let sleep_on_error = config.sync_notif_backoff.error_delay;
    tokio::task::spawn_local(async move {
        loop {
            let sleep_for = match send_sync_notifs(&db, &config).await {
                Err(e) => {
                    log::error!("PaymentSyncNeeded sendout job failed: {e}");
                    sleep_on_error
                }
                Ok(duration) => {
                    let sleep_duration = duration.unwrap_or(sleep_on_error);
                    log::debug!(
                        "PaymentSyncNeeded sendout job done, sleeping for {:?}",
                        sleep_duration
                    );
                    sleep_duration
                }
            };

            tokio::select! {
                _ = tokio::time::sleep(sleep_for) => { },
                _ = SYNC_NOTIFS_NOTIFY.notified() => { },
            }
        }
    });
}

async fn send_sync_requests_impl(db: DbExecutor) -> anyhow::Result<()> {
    let invoice_dao: InvoiceDao = db.as_dao();
    let debit_note_dao: DebitNoteDao = db.as_dao();

    let identities = typed::service(identity::BUS_ID)
        .call(ya_core_model::identity::List {})
        .await??;

    for IdentityInfo { node_id, .. } in identities {
        let mut peers = HashSet::<NodeId>::default();

        for invoice in invoice_dao.dangling(node_id).await? {
            peers.insert(invoice.recipient_id);
        }

        for debit_note in debit_note_dao.dangling(node_id).await? {
            peers.insert(debit_note.recipient_id);
        }

        for peer_id in peers {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            log::debug!("Sending PaymentSyncRequest to [{peer_id}]");
            let result = ya_net::from(node_id)
                .to(peer_id)
                .service(payment::public::BUS_ID)
                .call(PaymentSyncRequest)
                .timeout(Some(REMOTE_CALL_TIMEOUT))
                .await;

            match result {
                Err(_) => {
                    log::debug!("Couldn't deliver PaymentSyncRequest to [{peer_id}]: timeout");
                }
                Ok(Err(e)) => {
                    log::debug!("Couldn't deliver PaymentSyncRequest to [{peer_id}]: {e}");
                }
                Ok(Ok(_)) => {}
            }
        }
    }

    Ok(())
}

pub fn send_sync_requests(db: DbExecutor) {
    tokio::task::spawn_local(async move {
        if let Err(e) = send_sync_requests_impl(db).await {
            log::debug!("Failed to send PaymentSyncRequest: {e}");
        }
    });
}
