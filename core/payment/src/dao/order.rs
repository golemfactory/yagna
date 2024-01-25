use crate::dao::{activity, agreement, allocation};
use crate::error::DbResult;
use crate::models::order::{ReadObj, WriteObj};
use crate::schema::pay_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_invoice::dsl as invoice_dsl;
use crate::schema::pay_order::dsl;
use diesel::{
    self, BoolExpressionMethods, ExpressionMethods, JoinOnDsl, NullableExpressionMethods, QueryDsl,
    RunQueryDsl,
};
use ya_core_model::payment::local::{
    DebitNotePayment, InvoicePayment, PaymentTitle, SchedulePayment,
};
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

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
        do_with_transaction(self.pool, "order_dao_create", move |conn| {
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
        readonly_transaction(self.pool, "order_dao_get_many", move |conn| {
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
}
