use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use crate::error::{DbError, DbResult};
use crate::models::{PaymentEntity, TransactionEntity, TX_CONFIRMED};
use crate::schema::gnt_driver_payment::dsl;

use crate::schema::gnt_driver_transaction::dsl as tx_dsl;

use crate::utils::{payment_entity_to_status, PAYMENT_STATUS_OK};
use ya_core_model::driver::{PaymentConfirmation, PaymentStatus};
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

#[allow(unused)]
pub struct PaymentDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for PaymentDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> PaymentDao<'c> {
    pub async fn get(&self, invoice_id: String) -> DbResult<Option<PaymentEntity>> {
        do_with_transaction(self.pool, move |conn| {
            let payment: Option<PaymentEntity> = dsl::gnt_driver_payment
                .find(invoice_id)
                .first(conn)
                .optional()?;
            match payment {
                Some(payment) => Ok(Some(payment)),
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn get_pending_payments(&self, address: String) -> DbResult<Vec<PaymentEntity>> {
        do_with_transaction(self.pool, move |conn| {
            let payments: Vec<PaymentEntity> = dsl::gnt_driver_payment
                .filter(dsl::sender.eq(address))
                .filter(dsl::status.eq(crate::utils::PAYMENT_STATUS_NOT_YET))
                .order(dsl::payment_due_date.asc())
                .load(conn)?;
            Ok(payments)
        })
        .await
    }

    pub async fn get_payment_status(&self, invoice_id: String) -> DbResult<Option<PaymentStatus>> {
        //
        readonly_transaction(self.pool, move |conn| {
            let payment: PaymentEntity = match dsl::gnt_driver_payment
                .find(&invoice_id)
                .first(conn)
                .optional()?
            {
                Some(v) => v,
                None => return Ok(None),
            };
            let tx_id = payment.tx_id.clone();
            let mut status = payment_entity_to_status(&payment);
            if let PaymentStatus::Ok(ref mut confirmation) = &mut status {
                let tx_id = match tx_id {
                    Some(v) => v,
                    None => {
                        log::error!("invalid payment state (invoice={})", invoice_id);
                        return Ok(Some(PaymentStatus::Unknown));
                    }
                };

                let tx: TransactionEntity =
                    tx_dsl::gnt_driver_transaction.find(&tx_id).first(conn)?;
                if tx.status != TX_CONFIRMED {
                    return Ok(Some(PaymentStatus::NotYet));
                }
                let tx_hash = match tx.tx_hash {
                    Some(h) => hex::decode(h).map_err(|e| DbError::InvalidData(e.to_string()))?,
                    None => {
                        log::error!("invalid payment state (invoice={})", invoice_id);
                        return Ok(Some(PaymentStatus::Unknown));
                    }
                };
                *confirmation = PaymentConfirmation {
                    confirmation: tx_hash,
                };
            }

            Ok(Some(status))
        })
        .await
    }

    pub async fn insert(&self, payment: PaymentEntity) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::gnt_driver_payment)
                .values(payment)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_status(&self, invoice_id: String, status: i32) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::gnt_driver_payment.find(invoice_id))
                .set(dsl::status.eq(status))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_tx_id(&self, invoice_id: String, tx_id: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::gnt_driver_payment.find(invoice_id))
                .set((dsl::tx_id.eq(tx_id), dsl::status.eq(PAYMENT_STATUS_OK)))
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
