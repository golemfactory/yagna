use crate::error::{DbError, DbResult};
use crate::schema::{pay_payment, pay_payment_document};
use bigdecimal::BigDecimal;
use chrono::{NaiveDateTime, TimeZone, Utc};
use ya_client_model::payment as api_model;
use ya_client_model::payment::payment::Signature;
use ya_client_model::NodeId;
use ya_persistence::types::{BigDecimalField, Role};

#[derive(Debug, Identifiable, Insertable)]
#[table_name = "pay_payment"]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub peer_id: NodeId,
    pub payee_addr: String,
    pub payer_addr: String,
    pub payment_platform: String,
    pub role: Role,
    pub amount: BigDecimalField,
    pub details: Vec<u8>,
    pub send_payment: bool,
    pub signature: Option<Vec<u8>>,
    pub signed_bytes: Option<Vec<u8>>,
}

impl WriteObj {
    #[allow(clippy::too_many_arguments)]
    pub fn new_sent(
        payment_id: String,
        payer_id: NodeId,
        payee_id: NodeId,
        payer_addr: String,
        payee_addr: String,
        payment_platform: String,
        amount: BigDecimal,
        details: Vec<u8>,
        signature: Option<Vec<u8>>,
        signed_bytes: Option<Vec<u8>>,
    ) -> Self {
        Self {
            id: payment_id,
            owner_id: payer_id,
            peer_id: payee_id,
            payer_addr,
            payee_addr,
            payment_platform,
            role: Role::Requestor,
            amount: amount.into(),
            details,
            send_payment: true,
            signature,
            signed_bytes,
        }
    }

    pub fn new_received(
        payment: api_model::Payment,
        signature: Option<Vec<u8>>,
        signed_bytes: Option<Vec<u8>>,
    ) -> DbResult<Self> {
        let details = base64::decode(&payment.details)
            .map_err(|_| DbError::Query("Payment details is not valid base-64".to_string()))?;
        Ok(Self {
            id: payment.payment_id,
            owner_id: payment.payee_id,
            peer_id: payment.payer_id,
            payer_addr: payment.payer_addr,
            payee_addr: payment.payee_addr,
            payment_platform: payment.payment_platform,
            role: Role::Provider,
            amount: payment.amount.into(),
            details,
            send_payment: false,
            signature,
            signed_bytes,
        })
    }
}

#[derive(Queryable, Debug, Identifiable, Clone)]
#[table_name = "pay_payment"]
#[primary_key(id, owner_id)]
pub struct ReadObj {
    pub id: String,
    pub owner_id: NodeId,
    pub peer_id: NodeId,
    pub payee_addr: String,
    pub payer_addr: String,
    pub payment_platform: String,
    pub role: Role,
    pub amount: BigDecimalField,
    pub timestamp: NaiveDateTime,
    pub details: Vec<u8>,
    pub send_payment: bool,
    pub signature: Option<Vec<u8>>,
    pub signed_bytes: Option<Vec<u8>>,
}

impl ReadObj {
    pub fn payee_id(&self) -> NodeId {
        match self.role {
            Role::Provider => self.owner_id,
            Role::Requestor => self.peer_id,
        }
    }

    pub fn payer_id(&self) -> NodeId {
        match self.role {
            Role::Provider => self.peer_id,
            Role::Requestor => self.owner_id,
        }
    }

    pub fn into_api_model(self, document_payment: Vec<DocumentPayment>) -> api_model::Payment {
        let mut activity_payments = vec![];
        let mut agreement_payments = vec![];
        for dp in &document_payment {
            if let Some(activity_id) = &dp.activity_id {
                activity_payments.push(api_model::ActivityPayment {
                    activity_id: activity_id.clone(),
                    amount: dp.amount.0.clone(),
                    allocation_id: None,
                });
            } else {
                agreement_payments.push(api_model::AgreementPayment {
                    agreement_id: dp.agreement_id.clone(),
                    amount: dp.amount.0.clone(),
                    allocation_id: None,
                });
            }
        }
        api_model::Payment {
            payer_id: self.payer_id(),
            payee_id: self.payee_id(),
            payment_id: self.id,
            payer_addr: self.payer_addr,
            payee_addr: self.payee_addr,
            payment_platform: self.payment_platform,
            amount: self.amount.into(),
            timestamp: Utc.from_utc_datetime(&self.timestamp),
            activity_payments: activity_payments.into_iter().map(Into::into).collect(),
            agreement_payments: agreement_payments.into_iter().map(Into::into).collect(),
            details: base64::encode(&self.details),
        }
    }

    pub fn into_signed_api_model(
        self,
        document_payments: Vec<DocumentPayment>,
    ) -> api_model::Signed<api_model::Payment> {
        api_model::Signed {
            payload: self.clone().into_api_model(document_payments),
            signature: if self.signature.is_some() && self.signed_bytes.is_some() {
                Some(Signature {
                    signature: self.signature.unwrap(),
                    signed_bytes: self.signed_bytes.unwrap(),
                })
            } else {
                None
            },
        }
    }
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_payment_document"]
#[primary_key(owner_id, payment_id, agreement_id, activity_id)]
pub struct DocumentPayment {
    pub owner_id: NodeId,
    pub peer_id: NodeId,
    pub payment_id: String,
    pub agreement_id: String,
    pub invoice_id: Option<String>,
    pub activity_id: Option<String>,
    pub debit_note_id: Option<String>,
    pub amount: BigDecimalField,
}
