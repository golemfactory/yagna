use std::collections::HashMap;

use bigdecimal::BigDecimal;
use diesel::prelude::*;

use ya_core_model::payment::local::{
    DebitNotePayment, InvoicePayment, PaymentTitle, SchedulePayment,
};
use ya_core_model::NodeId;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::dao::{activity, agreement, allocation};
use crate::error::{DbError, DbResult};
use crate::models::batch::DbBatchOrderItem;
use crate::models::order::{ReadObj, WriteObj};
use crate::schema::pay_batch_order::dsl as odsl;
use crate::schema::pay_batch_order_item::dsl as oidsl;
use crate::schema::pay_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_invoice::dsl as invoice_dsl;

use crate::schema::pay_order::dsl;

pub struct OrderDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for OrderDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> OrderDao<'c> {
    pub async fn create(&self, msg: SchedulePayment, id: String, driver: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            match &msg.title {
                PaymentTitle::DebitNote(DebitNotePayment { activity_id, .. }) => {
                    activity::increase_amount_scheduled(
                        activity_id,
                        &msg.payer_id,
                        &msg.amount,
                        conn,
                    )?
                }
                PaymentTitle::Invoice(InvoicePayment { agreement_id, .. }) => {
                    agreement::increase_amount_scheduled(
                        agreement_id,
                        &msg.payer_id,
                        &msg.amount,
                        conn,
                    )?
                }
            };
            let order = WriteObj::new(msg, id, driver);
            allocation::spend_from_allocation(&order.allocation_id, &order.amount, conn)?;
            diesel::insert_into(dsl::pay_order)
                .values(order)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_many(&self, ids: Vec<String>, driver: String) -> DbResult<Vec<ReadObj>> {
        readonly_transaction(self.pool, move |conn| {
            let orders = dsl::pay_order
                .left_join(
                    invoice_dsl::pay_invoice.on(dsl::invoice_id
                        .eq(invoice_dsl::id.nullable())
                        .and(dsl::payer_id.eq(invoice_dsl::owner_id))),
                )
                .left_join(
                    debit_note_dsl::pay_debit_note.on(dsl::debit_note_id
                        .eq(debit_note_dsl::id.nullable())
                        .and(dsl::payer_id.eq(debit_note_dsl::owner_id))),
                )
                .filter(dsl::id.eq_any(ids))
                .filter(dsl::driver.eq(driver))
                .select((
                    dsl::id,
                    dsl::driver,
                    dsl::amount,
                    dsl::payee_id,
                    dsl::payer_id,
                    dsl::payee_addr,
                    dsl::payer_addr,
                    dsl::payment_platform,
                    dsl::invoice_id,
                    dsl::debit_note_id,
                    dsl::allocation_id,
                    dsl::is_paid,
                    invoice_dsl::agreement_id.nullable(),
                    debit_note_dsl::activity_id.nullable(),
                ))
                .load(conn)?;
            Ok(orders)
        })
        .await
    }

    pub async fn get_orders(
        &self,
        ids: Vec<String>,
        driver: String,
    ) -> DbResult<(Vec<ReadObj>, Vec<DbBatchOrderItem>)> {
        readonly_transaction(self.pool, move |conn| {
            let orders = dsl::pay_order
                .left_join(
                    invoice_dsl::pay_invoice.on(dsl::invoice_id
                        .eq(invoice_dsl::id.nullable())
                        .and(dsl::payer_id.eq(invoice_dsl::owner_id))),
                )
                .left_join(
                    debit_note_dsl::pay_debit_note.on(dsl::debit_note_id
                        .eq(debit_note_dsl::id.nullable())
                        .and(dsl::payer_id.eq(debit_note_dsl::owner_id))),
                )
                .filter(dsl::id.eq_any(&ids))
                .filter(dsl::driver.eq(&driver))
                .select((
                    dsl::id,
                    dsl::driver,
                    dsl::amount,
                    dsl::payee_id,
                    dsl::payer_id,
                    dsl::payee_addr,
                    dsl::payer_addr,
                    dsl::payment_platform,
                    dsl::invoice_id,
                    dsl::debit_note_id,
                    dsl::allocation_id,
                    dsl::is_paid,
                    invoice_dsl::agreement_id.nullable(),
                    debit_note_dsl::activity_id.nullable(),
                ))
                .load(conn)?;

            let batch_orders = super::batch::get_batch_orders(conn, &ids, &driver)?;

            Ok((orders, batch_orders))
        })
        .await
    }

    pub async fn get_batch_items(
        &self,
        owner_id: NodeId,
        platform: String,
    ) -> DbResult<HashMap<(String, String), BigDecimal>> {
        readonly_transaction(self.pool, move |conn| {
            let data: Vec<(String, String, String, bool)> = odsl::pay_batch_order
                .filter(
                    odsl::platform
                        .eq(&platform)
                        .and(odsl::owner_id.eq(owner_id)),
                )
                .inner_join(oidsl::pay_batch_order_item)
                .select((
                    odsl::payer_addr,
                    oidsl::payee_addr,
                    oidsl::amount,
                    oidsl::paid,
                ))
                .load(conn)?;

            data
                .into_iter()
                .map(|(payer_addr, payee_addr, amount, paid)| -> Result<((String, String), BigDecimal), DbError> {
                    Ok(((payer_addr, payee_addr), amount.parse().map_err(|e : bigdecimal::ParseBigDecimalError| DbError::Integrity(e.to_string()))?))
                })
                .collect()
        })
        .await
    }
}
