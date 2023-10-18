use std::time::Duration;

use chrono::Utc;
use tokio::sync::Notify;
use tokio_util::task::LocalPoolHandle;
use ya_client_model::{payment::Acceptance, NodeId};
use ya_core_model::{
    driver::{driver_bus_id, SignPayment},
    identity,
    payment::public::{AcceptDebitNote, AcceptInvoice, PaymentSync, SendPayment},
};
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{typed, RpcEndpoint};

use crate::dao::{DebitNoteDao, InvoiceDao, PaymentDao, SyncNotifsDao};

const SYNC_NOTIF_DELAY_0: Duration = Duration::from_secs(30);
const SYNC_NOTIF_RATIO: u32 = 6;
const SYNC_NOTIF_MAX_RETRIES: u32 = 7;

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

async fn send_sync_notifs(db: &DbExecutor) -> anyhow::Result<Option<Duration>> {
    let dao: SyncNotifsDao = db.as_dao();

    let exp_backoff = |n| SYNC_NOTIF_DELAY_0 * SYNC_NOTIF_RATIO.pow(n);
    let cutoff = Utc::now();

    let default_identity = typed::service(identity::BUS_ID)
        .call(ya_core_model::identity::Get::ByDefault {})
        .await??
        .ok_or_else(|| anyhow::anyhow!("No default identity"))?
        .node_id;

    let all_notifs = dao.list().await?;

    let next_wakeup = all_notifs
        .iter()
        .map(|entry| {
            let next_deadline = entry.timestamp + exp_backoff(entry.retries as _);
            next_deadline.and_utc()
        })
        .filter(|deadline| deadline > &cutoff)
        .min()
        .map(|ts| ts - cutoff)
        .and_then(|dur| dur.to_std().ok());

    let peers_to_notify = dao
        .list()
        .await?
        .into_iter()
        .filter(|entry| {
            let next_deadline = entry.timestamp + exp_backoff(entry.retries as _);
            next_deadline.and_utc() < cutoff && entry.retries <= SYNC_NOTIF_MAX_RETRIES as i32
        })
        .map(|entry| entry.id)
        .collect::<Vec<_>>();

    for peer in peers_to_notify {
        let msg = payment_sync(db, peer).await?;

        let result = ya_net::from(default_identity)
            .to(peer)
            .service(ya_core_model::payment::public::BUS_ID)
            .call(msg.clone())
            .await;

        if matches!(&result, Ok(Ok(_))) {
            mark_all_sent(db, msg).await?;
            dao.drop(peer).await?;
        } else {
            dao.increment_retry(peer, cutoff.naive_utc()).await?;
        }
    }

    Ok(next_wakeup)
}

lazy_static::lazy_static! {
    pub static ref SYNC_NOTIFS_NOTIFY: Notify = Notify::new();
}

pub fn send_sync_notifs_job(db: DbExecutor) {
    let pool = LocalPoolHandle::new(1);
    let default_sleep = Duration::from_secs(30);

    pool.spawn_pinned(move || async move {
        loop {
            let sleep_for = match send_sync_notifs(&db).await {
                Err(e) => {
                    log::error!("PaymentSyncNeeded sendout job failed: {e}");
                    default_sleep
                }
                Ok(duration) => {
                    log::trace!("PaymentSyncNeeded sendout job done");
                    duration.unwrap_or(default_sleep)
                }
            };

            tokio::select! {
                _ = tokio::time::sleep(sleep_for) => { },
                _ = SYNC_NOTIFS_NOTIFY.notified() => { },
            }
        }
    });
}
