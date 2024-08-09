#![allow(unused)]

use anyhow::anyhow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ya_core_model as model;
use ya_core_model::bus::GsbBindPoints;
use ya_core_model::payment::public::{
    AcceptDebitNote, AcceptInvoice, AcceptRejectError, Ack, CancelDebitNote, CancelError,
    CancelInvoice, PaymentSync, PaymentSyncError, PaymentSyncRequest, PaymentSyncWithBytes,
    RejectDebitNote, RejectInvoiceV2, SendDebitNote, SendError, SendInvoice, SendPayment,
    SendSignedPayment, BUS_ID,
};
use ya_payment::migrations;
use ya_payment::processor::PaymentProcessor;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::ServiceBinder;

#[derive(Clone)]
pub struct FakePayment {
    name: String,
    testdir: PathBuf,

    db: DbExecutor,
    gsb: GsbBindPoints,
}

impl FakePayment {
    pub fn new(name: &str, testdir: &Path) -> Self {
        let db = Self::create_db(testdir, name).unwrap();
        FakePayment {
            name: name.to_string(),
            testdir: testdir.to_path_buf(),
            db,
            gsb: GsbBindPoints::default().service(model::payment::local::BUS_SERVICE_NAME),
        }
    }

    fn create_db(_testdir: &Path, name: &str) -> anyhow::Result<DbExecutor> {
        let db = DbExecutor::in_memory(&format!("{name}.payment.db"))
            .map_err(|e| anyhow!("Failed to create db [{name:?}]. Error: {e}"))?;
        db.apply_migration(migrations::run_with_output)?;
        Ok(db)
    }

    pub fn with_prefixed_gsb(mut self, gsb: Option<GsbBindPoints>) -> Self {
        self.gsb = gsb
            .unwrap_or_default()
            .service(model::payment::local::BUS_SERVICE_NAME);
        self
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        log::info!("FakePayment ({}) - binding GSB", self.name);

        let gsb = self.gsb.clone();
        ServiceBinder::new(gsb.public_addr(), &self.db, ())
            .bind(send_debit_note)
            .bind(accept_debit_note)
            .bind(reject_debit_note)
            .bind(cancel_debit_note)
            .bind(send_invoice)
            .bind(accept_invoice)
            .bind(reject_invoice)
            .bind(cancel_invoice)
            .bind(sync_request)
            .bind(send_payment)
            .bind(send_payment_with_bytes)
            .bind(sync_payment)
            .bind(sync_payment_with_bytes);

        Ok(())
    }
}

async fn send_debit_note(
    db: DbExecutor,
    sender_id: String,
    msg: SendDebitNote,
) -> Result<Ack, SendError> {
    Ok(Ack {})
}

async fn accept_debit_note(
    db: DbExecutor,
    sender_id: String,
    msg: AcceptDebitNote,
) -> Result<Ack, AcceptRejectError> {
    Ok(Ack {})
}

async fn reject_debit_note(
    db: DbExecutor,
    sender: String,
    msg: RejectDebitNote,
) -> Result<Ack, AcceptRejectError> {
    Ok(Ack {})
}

async fn cancel_debit_note(
    db: DbExecutor,
    sender: String,
    msg: CancelDebitNote,
) -> Result<Ack, CancelError> {
    Ok(Ack {})
}

// *************************** INVOICE ****************************

async fn send_invoice(
    db: DbExecutor,
    sender_id: String,
    msg: SendInvoice,
) -> Result<Ack, SendError> {
    Ok(Ack {})
}

async fn accept_invoice(
    db: DbExecutor,
    sender_id: String,
    msg: AcceptInvoice,
) -> Result<Ack, AcceptRejectError> {
    Ok(Ack {})
}

async fn reject_invoice(
    db: DbExecutor,
    sender_id: String,
    msg: RejectInvoiceV2,
) -> Result<Ack, AcceptRejectError> {
    Ok(Ack {})
}

async fn cancel_invoice(
    db: DbExecutor,
    sender_id: String,
    msg: CancelInvoice,
) -> Result<Ack, CancelError> {
    Ok(Ack {})
}

async fn send_payment(
    db: DbExecutor,
    sender_id: String,
    msg: SendPayment,
) -> Result<Ack, SendError> {
    Ok(Ack {})
}

async fn send_payment_with_bytes(
    db: DbExecutor,
    sender_id: String,
    msg: SendSignedPayment,
) -> Result<Ack, SendError> {
    Ok(Ack {})
}

async fn sync_payment(
    db: DbExecutor,
    sender_id: String,
    msg: PaymentSync,
) -> Result<Ack, PaymentSyncError> {
    Ok(Ack {})
}

async fn sync_payment_with_bytes(
    db: DbExecutor,
    sender_id: String,
    msg: PaymentSyncWithBytes,
) -> Result<Ack, PaymentSyncError> {
    Ok(Ack {})
}

async fn sync_request(
    db: DbExecutor,
    sender_id: String,
    msg: PaymentSyncRequest,
) -> Result<Ack, SendError> {
    Ok(Ack {})
}
