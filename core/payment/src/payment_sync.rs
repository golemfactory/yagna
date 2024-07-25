use std::sync::Arc;
use std::{collections::HashSet, time::Duration};
use ya_client_model::payment::Payment;

use crate::Config;

use chrono::Utc;
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
use ya_service_bus::{timeout::IntoTimeoutFuture, typed, Error, RpcEndpoint};

use crate::dao::{DebitNoteDao, InvoiceDao, InvoiceEventDao, PaymentDao, SyncNotifsDao};

const REMOTE_CALL_TIMEOUT: Duration = Duration::from_secs(30);

fn remove_allocation_ids_from_payment(payment: &Payment) -> Payment {
    // We remove allocation ID from syncs because allocations are not transferred to peers and
    // their IDs would be unknown to the recipient.
    let mut payment = payment.clone();
    for agreement_payment in &mut payment.agreement_payments.iter_mut() {
        agreement_payment.allocation_id = None;
    }

    for activity_payment in &mut payment.activity_payments.iter_mut() {
        activity_payment.allocation_id = None;
    }

    payment
}

async fn payment_sync(
    db: &DbExecutor,
    current_node_id: NodeId,
    peer_id: NodeId,
) -> anyhow::Result<(PaymentSync, PaymentSyncWithBytes)> {
    let payment_dao: PaymentDao = db.as_dao();
    let invoice_dao: InvoiceDao = db.as_dao();
    let debit_note_dao: DebitNoteDao = db.as_dao();
    let invoice_event_dao: InvoiceEventDao = db.as_dao();

    let mut payments = Vec::default();
    let mut payments_canonicalized = Vec::default();
    for payment in payment_dao.list_unsent(Some(peer_id)).await? {
        let platform_components = payment.payment_platform.split('-').collect::<Vec<_>>();
        let driver = &platform_components[0];

        let payment = remove_allocation_ids_from_payment(&payment);

        let signature = typed::service(driver_bus_id(driver))
            .send(SignPayment(payment.clone()))
            .await??;
        payments.push(SendPayment::new(payment.clone(), signature));

        let signature_canonicalized = typed::service(driver_bus_id(driver))
            .send(SignPaymentCanonicalized(payment.clone()))
            .await??;
        payments_canonicalized.push(SendSignedPayment::new(payment, signature_canonicalized));
    }

    let mut invoice_accepts = Vec::default();
    for invoice in invoice_dao
        .unsent_accepted(current_node_id, peer_id)
        .await?
    {
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
    for invoice in invoice_dao
        .unsent_rejected(current_node_id, peer_id)
        .await?
    {
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
    for debit_note in debit_note_dao
        .unsent_accepted(current_node_id, peer_id)
        .await?
    {
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
        payment_dao
            .mark_sent(payment_send.payment.payment_id, owner_id)
            .await?;
    }

    for invoice_accept in msg.invoice_accepts {
        invoice_dao
            .mark_accept_sent(invoice_accept.invoice_id, owner_id)
            .await?;
    }

    for invoice_reject in msg.invoice_rejects {
        invoice_dao
            .mark_reject_sent(invoice_reject.invoice_id, owner_id)
            .await?;
    }

    for debit_note_accept in msg.debit_note_accepts {
        debit_note_dao
            .mark_accept_sent(debit_note_accept.debit_note_id, owner_id)
            .await?;
    }

    Ok(())
}

async fn send_sync_notifs(db: &DbExecutor, config: &Config) -> anyhow::Result<Option<Duration>> {
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
            let next_deadline = entry.last_ping + exp_backoff(entry.retries as _);
            next_deadline.and_utc() <= cutoff && entry.retries <= backoff_config.max_retries as i32
        })
        .map(|entry| entry.id)
        .collect::<Vec<_>>();

    for peer in peers_to_notify {
        // FIXME: We should iterate over all identities present in the current instance or make
        // payment_sync return a mapping identity -> msg and use the returned identity as the
        // sender, or store notifying identity in SyncNotifsDao.
        // Currently we assume that everything is sent from the default identity.
        let (msg, msg_with_bytes) = payment_sync(db, default_identity, peer).await?;

        log::debug!("Sending PaymentSync as [{default_identity}] to [{peer}].");
        let mut result = ya_net::from(default_identity)
            .to(peer)
            .service(ya_core_model::payment::public::BUS_ID)
            .call(msg_with_bytes.clone())
            .await;

        log::debug!("Sending PaymentSync as [{default_identity}] to [{peer}] result: {result:?}");

        // PaymentSyncWithBytes is newer message that won't always be supported, but it contains
        // signatures that are crutial for clients that do support this message and rely on them
        // for payment verification.
        // For this reason we will try to send PaymentSyncWithBytes first and send the older
        // PaymentSync only if the new message is not supported.
        //
        // Manual tests on centralnet show that the following errors are returned:
        // if the endpoint is not supported
        //  Err(RemoteError("/net/<peer_id>/payment/PaymentSyncWithBytes", "GSB failure: Bad request: endpoint address not found"))
        // if the peer is not available
        //  Err(RemoteError("/net/<peer_id>/payment/PaymentSyncWithBytes", "Bad request: endpoint address not found"))
        // We'll Use presence of "GSB failure" message to distinguish them.
        //
        // We cannot just use any RemoteError or 'Bad request' as an indicator that old message
        // should be sent, becaues that could cause sending not signed messages to newer clients in
        // case of transient errors.
        // TODO: is there any better way to know if the peer is connected but the endpoint is not
        // handled?
        if matches!(&result, Err(Error::RemoteError(_, e)) if e.contains("GSB failure: Bad request: endpoint address not found"))
        {
            log::debug!("Sending PaymentSync as [{default_identity}] to [{peer}]: PaymentSyncWithBytes not supported, falling back to PaymentSync.");
            result = ya_net::from(default_identity)
                .to(peer)
                .service(ya_core_model::payment::public::BUS_ID)
                .call(msg.clone())
                .await;
        }

        if matches!(&result, Ok(Ok(_))) {
            log::debug!("Delivered PaymentSync to [{peer}] as [{default_identity}].");
            mark_all_sent(db, default_identity, msg).await?;
            dao.drop(peer).await?;
        } else {
            let err = match result {
                Err(x) => x.to_string(),
                Ok(Err(x)) => x.to_string(),
                Ok(Ok(_)) => unreachable!(),
            };
            log::debug!("Couldn't deliver PaymentSync to [{peer}] as [{default_identity}]: {err}");
            dao.increment_retry(peer, cutoff.naive_utc()).await?;
        }
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
