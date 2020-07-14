use diesel::{self, ExpressionMethods, QueryDsl, RunQueryDsl};

use crate::models::PaymentEntity;
use crate::schema::gnt_driver_payment::dsl;

use crate::utils::PAYMENT_STATUS_OK;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::dao::DbResult;

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

    pub async fn insert(&self, payment: PaymentEntity) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::gnt_driver_payment)
                .values(payment)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_status(&self, order_id: String, status: i32) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::gnt_driver_payment.find(order_id))
                .set(dsl::status.eq(status))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_tx_id(&self, order_id: String, tx_id: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::gnt_driver_payment.find(order_id))
                .set((dsl::tx_id.eq(tx_id), dsl::status.eq(PAYMENT_STATUS_OK)))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_by_tx_id(&self, tx_id: String) -> DbResult<Vec<PaymentEntity>> {
        do_with_transaction(self.pool, move |conn| {
            let payments: Vec<PaymentEntity> = dsl::gnt_driver_payment
                .filter(dsl::tx_id.eq(tx_id))
                .load(conn)?;
            Ok(payments)
        })
        .await
    }
}
