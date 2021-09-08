use crate::dao::{activity, agreement, allocation};
use crate::error::DbResult;
use crate::models::order::{BatchOrder, ReadObj, WriteObj};
use crate::schema::pay_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_invoice::dsl as invoice_dsl;
use crate::schema::pay_order::dsl;
use bigdecimal::{BigDecimal, ToPrimitive};
use diesel::{
    self, insert_into, BoolExpressionMethods, ExpressionMethods, JoinOnDsl,
    NullableExpressionMethods, QueryDsl, RunQueryDsl,
};
use std::collections::HashMap;
use uuid::Uuid;
use ya_core_model::payment::local::{
    DebitNotePayment, InvoicePayment, PaymentTitle, SchedulePayment,
};
use ya_core_model::NodeId;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};
use ya_persistence::types::BigDecimalField;

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

    pub async fn new_batch_order(
        &self,
        owner_id: NodeId,
        payer_addr: String,
        platform: String,
        items: HashMap<String, (BigDecimal, HashMap<NodeId, String>)>,
    ) -> DbResult<String> {
        do_with_transaction(self.pool, move |conn| {
            let order_id = Uuid::new_v4().to_string();
            {
                use crate::schema::pay_batch_order::dsl;

                let total_amount: BigDecimal = items.iter().map(|(_, (amount, _))| amount).sum();

                let v = insert_into(dsl::pay_batch_order)
                    .values((
                        dsl::id.eq(&order_id),
                        dsl::payer_addr.eq(payer_addr),
                        dsl::platform.eq(platform),
                        dsl::total_amount.eq(total_amount.to_f32()),
                    ))
                    .execute(conn)?;
            }
            {
                use crate::schema::pay_batch_order_item::dsl;

                for (payee_addr, (amount, payments)) in items {
                    insert_into(dsl::pay_batch_order_item)
                        .values((
                            dsl::id.eq(&order_id),
                            dsl::payee_addr.eq(&payee_addr),
                            dsl::amount.eq(BigDecimalField(amount)),
                        ))
                        .execute(conn)?;
                    for (payee_id, json) in payments {
                        use crate::schema::pay_batch_order_item_payment::dsl;

                        insert_into(dsl::pay_batch_order_item_payment)
                            .values((
                                dsl::id.eq(&order_id),
                                dsl::payee_addr.eq(&payee_addr),
                                dsl::payee_id.eq(payee_id),
                                dsl::json.eq(json),
                            ))
                            .execute(conn)?;
                    }
                }
            }

            Ok(order_id)
        })
        .await
    }
}
