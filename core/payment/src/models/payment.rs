use crate::error::{DbError, DbResult};
use crate::schema::{pay_activity_payment, pay_agreement_payment, pay_payment};
use bigdecimal::BigDecimal;
use chrono::{NaiveDateTime, TimeZone, Utc};
use uuid::Uuid;
use ya_client_model::payment as api_model;
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
}

impl WriteObj {
    pub fn new_sent(
        payer_id: NodeId,
        payee_id: NodeId,
        payer_addr: String,
        payee_addr: String,
        payment_platform: String,
        amount: BigDecimal,
        details: Vec<u8>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            owner_id: payer_id,
            peer_id: payee_id,
            payer_addr,
            payee_addr,
            payment_platform,
            role: Role::Requestor,
            amount: amount.into(),
            details,
        }
    }

    pub fn new_received(payment: api_model::Payment) -> DbResult<Self> {
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
        })
    }
}

#[derive(Queryable, Debug, Identifiable)]
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

    pub fn into_api_model(
        self,
        activity_payments: Vec<ActivityPayment>,
        agreement_payments: Vec<AgreementPayment>,
    ) -> api_model::Payment {
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
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_activity_payment"]
#[primary_key(payment_id, activity_id, owner_id)]
pub struct ActivityPayment {
    pub payment_id: String,
    pub activity_id: String,
    pub owner_id: NodeId,
    pub amount: BigDecimalField,
    pub allocation_id: Option<String>,
}

impl From<ActivityPayment> for api_model::ActivityPayment {
    fn from(ap: ActivityPayment) -> Self {
        Self {
            activity_id: ap.activity_id,
            amount: ap.amount.0,
            allocation_id: ap.allocation_id,
        }
    }
}

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_agreement_payment"]
#[primary_key(payment_id, agreement_id, owner_id)]
pub struct AgreementPayment {
    pub payment_id: String,
    pub agreement_id: String,
    pub owner_id: NodeId,
    pub amount: BigDecimalField,
    pub allocation_id: Option<String>,
}

impl From<AgreementPayment> for api_model::AgreementPayment {
    fn from(ap: AgreementPayment) -> Self {
        Self {
            agreement_id: ap.agreement_id,
            amount: ap.amount.0,
            allocation_id: ap.allocation_id,
        }
    }
}
