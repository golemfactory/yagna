/*
    Data access object for payment, linking `PaymentEntity` with `payment`
*/

// External crates
use diesel::{self, ExpressionMethods, QueryDsl, RunQueryDsl};

// Workspace uses
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

// Local uses
use crate::db::models::PAYMENT_STATUS_ACCEPTED;
use crate::{
    dao::DbResult,
    db::{
        models::{Network, PaymentEntity, PAYMENT_STATUS_NOT_YET, PAYMENT_STATUS_OK},
        schema::{payment, payment::dsl, transaction},
    },
};

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
    pub async fn get_pending_payments(
        &self,
        address: String,
        network: Network,
    ) -> DbResult<Vec<PaymentEntity>> {
        readonly_transaction(self.pool, move |conn| {
            let payments: Vec<PaymentEntity> = dsl::payment
                .filter(dsl::sender.eq(address))
                .filter(dsl::status.eq(PAYMENT_STATUS_NOT_YET))
                .filter(dsl::network.eq(network))
                .order(dsl::payment_due_date.asc())
                .load(conn)?;
            Ok(payments)
        })
        .await
    }

    pub async fn get_accpeted_payments(
        &self,
        address: String,
        network: Network,
    ) -> DbResult<Vec<PaymentEntity>> {
        readonly_transaction(self.pool, move |conn| {
            let payments: Vec<PaymentEntity> = dsl::payment
                .filter(dsl::sender.eq(address))
                .filter(dsl::status.eq(PAYMENT_STATUS_ACCEPTED))
                .filter(dsl::network.eq(network))
                .order(dsl::payment_due_date.asc())
                .load(conn)?;
            Ok(payments)
        })
        .await
    }

    pub async fn insert(&self, payment: PaymentEntity) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::payment)
                .values(payment)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_status(&self, order_id: String, status: i32) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::payment.find(order_id))
                .set(dsl::status.eq(status))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn update_tx_id(&self, order_id: String, tx_id: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::payment.find(order_id))
                .set((dsl::tx_id.eq(tx_id), dsl::status.eq(PAYMENT_STATUS_OK)))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_by_tx_id(&self, tx_id: String) -> DbResult<Vec<PaymentEntity>> {
        readonly_transaction(self.pool, move |conn| {
            let payments: Vec<PaymentEntity> =
                dsl::payment.filter(dsl::tx_id.eq(tx_id)).load(conn)?;
            Ok(payments)
        })
        .await
    }

    pub async fn get_first_by_tx_hash(&self, tx_hash: String) -> DbResult<PaymentEntity> {
        readonly_transaction(self.pool, move |conn| {
            let payments: PaymentEntity = dsl::payment
                .inner_join(transaction::table)
                .select(payment::all_columns)
                .filter(transaction::dsl::tx_hash.eq(tx_hash))
                .first(conn)?;
            Ok(payments)
        })
        .await
    }
}
