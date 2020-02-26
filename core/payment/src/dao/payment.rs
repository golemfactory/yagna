use crate::error::DbResult;
use crate::models::*;
use crate::schema::pay_payment::dsl;
use crate::schema::pay_payment_x_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_payment_x_invoice::dsl as invoice_dsl;
use bigdecimal::BigDecimal;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use std::collections::HashMap;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};
use ya_persistence::types::{BigDecimalField, Summable};

pub struct PaymentDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for PaymentDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> PaymentDao<'c> {
    pub async fn create(&self, payment: NewPayment) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let debit_note_ids = payment.debit_note_ids;
            let invoice_ids = payment.invoice_ids;
            let payment = payment.payment;
            let payment_id = payment.id.clone();

            diesel::insert_into(dsl::pay_payment)
                .values(payment)
                .execute(conn)?;

            debit_note_ids.into_iter().try_for_each(|debit_note_id| {
                let payment_id = payment_id.clone();
                diesel::insert_into(debit_note_dsl::pay_payment_x_debit_note)
                    .values(PaymentXDebitNote {
                        payment_id,
                        debit_note_id,
                    })
                    .execute(conn)
                    .map(|_| ())
            })?;

            invoice_ids.into_iter().try_for_each(|invoice_id| {
                let payment_id = payment_id.clone();
                diesel::insert_into(invoice_dsl::pay_payment_x_invoice)
                    .values(PaymentXInvoice {
                        payment_id,
                        invoice_id,
                    })
                    .execute(conn)
                    .map(|_| ())
            })?;

            Ok(())
        })
        .await
    }

    pub async fn get(&self, payment_id: String) -> DbResult<Option<Payment>> {
        do_with_transaction(self.pool, move |conn| {
            let payment: Option<BarePayment> = dsl::pay_payment
                .find(payment_id.clone())
                .first(conn)
                .optional()?;
            match payment {
                Some(payment) => {
                    let debit_note_ids = debit_note_dsl::pay_payment_x_debit_note
                        .select(debit_note_dsl::debit_note_id)
                        .filter(debit_note_dsl::payment_id.eq(payment_id.clone()))
                        .load(conn)?;
                    let invoice_ids = invoice_dsl::pay_payment_x_invoice
                        .select(invoice_dsl::invoice_id)
                        .filter(invoice_dsl::payment_id.eq(payment_id))
                        .load(conn)?;
                    Ok(Some(Payment {
                        payment,
                        debit_note_ids,
                        invoice_ids,
                    }))
                }
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn get_sent(&self, payer_id: String) -> DbResult<Vec<Payment>> {
        do_with_transaction(self.pool, move |conn| {
            let payments = dsl::pay_payment
                .filter(dsl::payer_id.eq(payer_id.clone()))
                .load(conn)?;
            let debit_notes = debit_note_dsl::pay_payment_x_debit_note
                .inner_join(dsl::pay_payment)
                .filter(dsl::payer_id.eq(payer_id.clone()))
                .select(crate::schema::pay_payment_x_debit_note::all_columns)
                .load(conn)?;
            let invoices = invoice_dsl::pay_payment_x_invoice
                .inner_join(dsl::pay_payment)
                .filter(dsl::payer_id.eq(payer_id))
                .select(crate::schema::pay_payment_x_invoice::all_columns)
                .load(conn)?;
            Ok(join_payments_with_debit_notes_and_payments(
                payments,
                debit_notes,
                invoices,
            ))
        })
        .await
    }

    pub async fn get_received(&self, payee_id: String) -> DbResult<Vec<Payment>> {
        do_with_transaction(self.pool, move |conn| {
            let payments = dsl::pay_payment
                .filter(dsl::payee_id.eq(payee_id.clone()))
                .load(conn)?;
            let debit_notes = debit_note_dsl::pay_payment_x_debit_note
                .inner_join(dsl::pay_payment)
                .filter(dsl::payee_id.eq(payee_id.clone()))
                .select(crate::schema::pay_payment_x_debit_note::all_columns)
                .load(conn)?;
            let invoices = invoice_dsl::pay_payment_x_invoice
                .inner_join(dsl::pay_payment)
                .filter(dsl::payee_id.eq(payee_id))
                .select(crate::schema::pay_payment_x_invoice::all_columns)
                .load(conn)?;
            Ok(join_payments_with_debit_notes_and_payments(
                payments,
                debit_notes,
                invoices,
            ))
        })
        .await
    }

    pub async fn get_transaction_balance(&self, payer_id: String) -> DbResult<BigDecimal> {
        do_with_transaction(self.pool, move |conn| {
            let amounts: Vec<BigDecimalField> = dsl::pay_payment
                .select(dsl::amount)
                .filter(dsl::payer_id.eq(payer_id))
                .load(conn)?;
            Ok(amounts.sum())
        })
        .await
    }
}

fn join_payments_with_debit_notes_and_payments(
    payments: Vec<BarePayment>,
    debit_notes: Vec<PaymentXDebitNote>,
    invoices: Vec<PaymentXInvoice>,
) -> Vec<Payment> {
    let mut debit_notes_map =
        debit_notes
            .into_iter()
            .fold(HashMap::new(), |mut map, debit_note| {
                map.entry(debit_note.payment_id)
                    .or_insert_with(Vec::new)
                    .push(debit_note.debit_note_id);
                map
            });
    let mut invoices_map = invoices
        .into_iter()
        .fold(HashMap::new(), |mut map, invoice| {
            map.entry(invoice.payment_id)
                .or_insert_with(Vec::new)
                .push(invoice.invoice_id);
            map
        });
    payments
        .into_iter()
        .map(|payment| Payment {
            debit_note_ids: debit_notes_map.remove(&payment.id).unwrap_or(vec![]),
            invoice_ids: invoices_map.remove(&payment.id).unwrap_or(vec![]),
            payment,
        })
        .collect()
}
