use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use crate::error::DbResult;
use crate::models::PaymentEntity;
use crate::schema::gnt_driver_payment::dsl;

use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

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
                .find(invoice_id.clone())
                .first(conn)
                .optional()?;
            match payment {
                Some(payment) => Ok(Some(payment)),
                None => Ok(None),
            }
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

    pub async fn update_status(
        &self,
        invoice_id: String,
        status: i32,
        tx_id: Option<String>,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::gnt_driver_payment.find(invoice_id.clone()))
                .set((dsl::status.eq(status), dsl::tx_id.eq(tx_id)))
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
