#![allow(unused)]

use anyhow::anyhow;
use bigdecimal::BigDecimal;
use chrono::{Duration, Utc};
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use ya_agreement_utils::AgreementView;
use ya_client_model::market::Agreement;
use ya_client_model::payment::allocation::PaymentPlatformEnum;
use ya_client_model::payment::{DocumentStatus, Invoice, NewAllocation};
use ya_client_model::NodeId;
use ya_core_model as model;
use ya_core_model::bus::GsbBindPoints;
use ya_core_model::payment::public;
use ya_core_model::payment::public::{
    AcceptDebitNote, AcceptInvoice, AcceptRejectError, Ack, CancelDebitNote, CancelError,
    CancelInvoice, PaymentSync, PaymentSyncError, PaymentSyncRequest, PaymentSyncWithBytes,
    RejectDebitNote, RejectInvoiceV2, SendDebitNote, SendError, SendInvoice, SendPayment,
    SendSignedPayment, BUS_ID,
};
use ya_payment::migrations;
use ya_payment::processor::PaymentProcessor;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;
use ya_service_bus::typed::{Endpoint, ServiceBinder};
use ya_service_bus::RpcMessage;

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

    /// Function binds new GSB handler to the given message.
    /// It returns Receiver that can be used to inspect the messages and make assertions.
    /// GSB will always return `result` passed in parameter back to the caller.
    /// Function overrides previous handler, so only one Receiver at the same time can be used.
    pub fn message_channel<T>(
        &self,
        result: Result<T::Item, T::Error>,
    ) -> mpsc::UnboundedReceiver<(NodeId, T)>
    where
        T: RpcMessage,
        T::Item: Clone,
        T::Error: Clone + Display,
    {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.override_gsb_public()
            .bind(move |_db: DbExecutor, sender_id: String, msg: T| {
                let result = result.clone();
                let sender = sender.clone();
                async move {
                    let id = NodeId::from_str(&sender_id).unwrap();
                    let _ = sender.send((id, msg)).map_err(|_e| {
                        log::error!(
                            "[FakePayment] Unable to send message '{}' to channel.",
                            T::ID
                        );
                    });
                    result
                }
            });
        receiver
    }

    pub fn override_gsb_public(&self) -> ServiceBinder<DbExecutor, ()> {
        ServiceBinder::new(self.gsb.public_addr(), &self.db, ())
    }

    pub fn override_gsb_local(&self) -> ServiceBinder<DbExecutor, ()> {
        ServiceBinder::new(self.gsb.local_addr(), &self.db, ())
    }

    /// Unbinds GSB public endpoint.
    /// TODO: it would be nice to be able to unbind each message separately,
    ///       but GSB doesn't allow this; it can only unbind whole GSB prefix.
    pub async fn unbind_public(&self) {
        bus::unbind(self.gsb.public_addr()).await;
    }

    pub async fn unbind_local(&self) {
        bus::unbind(self.gsb.local_addr()).await;
    }

    pub fn gsb_local_endpoint(&self) -> Endpoint {
        self.gsb.local()
    }

    pub fn gsb_public_endpoint(&self) -> Endpoint {
        self.gsb.public()
    }

    fn platform_from(agreement: &Agreement) -> anyhow::Result<String> {
        let view = AgreementView::try_from(agreement)?;
        Ok(view.pointer_typed("/demand/properties/golem/com/payment/chosen-platform")?)
    }

    pub fn fake_invoice(agreement: &Agreement, amount: BigDecimal) -> anyhow::Result<Invoice> {
        let platform = Self::platform_from(agreement)?;
        Ok(Invoice {
            invoice_id: Uuid::new_v4().to_string(),
            issuer_id: agreement.offer.provider_id,
            recipient_id: agreement.demand.requestor_id,
            payee_addr: agreement.offer.provider_id.to_string(),
            payer_addr: agreement.demand.requestor_id.to_string(),
            payment_platform: platform,
            timestamp: Utc::now(),
            agreement_id: agreement.agreement_id.to_string(),
            activity_ids: vec![],
            amount,
            payment_due_date: Utc::now() + Duration::seconds(10),
            status: DocumentStatus::Issued,
        })
    }

    pub fn default_allocation(
        agreement: &Agreement,
        amount: BigDecimal,
    ) -> anyhow::Result<NewAllocation> {
        let platform = Self::platform_from(agreement)?;
        let payment_platform = PaymentPlatformEnum::PaymentPlatformName(platform);

        Ok(NewAllocation {
            address: None, // Use default address (i.e. identity)
            payment_platform: Some(payment_platform.clone()),
            total_amount: amount,
            timeout: None,
            make_deposit: false,
            deposit: None,
            extend_timeout: None,
        })
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
