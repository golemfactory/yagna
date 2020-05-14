use crate::dao::activity;
use crate::dao::agreement;
use crate::dao::allocation;
use crate::dao::debit_note;
use crate::dao::invoice;
use crate::error::DbResult;
use crate::models::payment::{PaymentXDebitNote, PaymentXInvoice, ReadObj, WriteObj};
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_payment::dsl;
use crate::schema::pay_payment_x_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_payment_x_invoice::dsl as invoice_dsl;
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::{
    BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl, RunQueryDsl,
};
use std::collections::HashMap;
use ya_client_model::payment::{DocumentStatus, Payment};
use ya_core_model::ethaddr::NodeId;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};
use ya_persistence::types::Role;

pub struct PaymentDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for PaymentDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

// FIXME: This could probably be a function
macro_rules! query {
    () => {
        dsl::pay_payment
            .inner_join(
                agreement_dsl::pay_agreement.on(dsl::owner_id
                    .eq(agreement_dsl::owner_id)
                    .and(dsl::agreement_id.eq(agreement_dsl::id))),
            )
            .select((
                dsl::id,
                dsl::owner_id,
                dsl::role,
                dsl::agreement_id,
                dsl::allocation_id,
                dsl::amount,
                dsl::timestamp,
                dsl::details,
                agreement_dsl::peer_id,
                agreement_dsl::payee_addr,
                agreement_dsl::payer_addr,
            ))
    };
}

impl<'c> PaymentDao<'c> {
    async fn insert(
        &self,
        payment: WriteObj,
        debit_note_ids: Vec<String>,
        invoice_ids: Vec<String>,
    ) -> DbResult<()> {
        let payment_id = payment.id.clone();
        let owner_id = payment.owner_id.clone();
        let allocation_id = payment.allocation_id.clone();
        let agreement_id = payment.agreement_id.clone();
        let amount = payment.amount.clone();

        do_with_transaction(self.pool, move |conn| {
            // Insert payment
            diesel::insert_into(dsl::pay_payment)
                .values(payment)
                .execute(conn)?;

            // Insert debit note relations
            debit_note_ids.iter().try_for_each(|debit_note_id| {
                let payment_id = payment_id.clone();
                let debit_note_id = debit_note_id.clone();
                let owner_id = owner_id.clone();
                diesel::insert_into(debit_note_dsl::pay_payment_x_debit_note)
                    .values(PaymentXDebitNote {
                        payment_id,
                        debit_note_id,
                        owner_id,
                    })
                    .execute(conn)
                    .map(|_| ())
            })?;

            // Insert invoice relations
            invoice_ids.iter().try_for_each(|invoice_id| {
                let payment_id = payment_id.clone();
                let invoice_id = invoice_id.clone();
                let owner_id = owner_id.clone();
                diesel::insert_into(invoice_dsl::pay_payment_x_invoice)
                    .values(PaymentXInvoice {
                        payment_id,
                        invoice_id,
                        owner_id,
                    })
                    .execute(conn)
                    .map(|_| ())
            })?;

            // Update spent & remaining amount for allocation (if applicable)
            if let Some(allocation_id) = &allocation_id {
                allocation::spend_from_allocation(allocation_id, &amount, conn)?;
            }

            // Update total paid amount for agreement
            agreement::increase_amount_paid(&agreement_id, &owner_id, &amount, conn)?;

            // Update total paid amount for activities
            let amounts =
                debit_note::get_paid_amount_per_activity(&debit_note_ids, &owner_id, conn)?;
            activity::set_amounts_paid(&amounts, &owner_id, conn)?;

            // Set 'SETTLED' status for all invoices and debit notes
            debit_note::update_status(&debit_note_ids, &owner_id, &DocumentStatus::Settled, conn)?;
            invoice::update_status(&invoice_ids, &owner_id, &DocumentStatus::Settled, conn)?;

            Ok(())
        })
        .await
    }

    pub async fn create_new(
        &self,
        payer_id: NodeId,
        agreement_id: String,
        allocation_id: String,
        amount: BigDecimal,
        details: Vec<u8>,
        debit_note_ids: Vec<String>,
        invoice_ids: Vec<String>,
    ) -> DbResult<String> {
        let payment = WriteObj::new_sent(payer_id, agreement_id, allocation_id, amount, details);
        let payment_id = payment.id.clone();
        self.insert(payment, debit_note_ids, invoice_ids).await?;
        Ok(payment_id)
    }

    pub async fn insert_received(&self, payment: Payment, payee_id: NodeId) -> DbResult<()> {
        let debit_note_ids = payment.debit_note_ids.clone().unwrap_or(vec![]);
        let invoice_ids = payment.invoice_ids.clone().unwrap_or(vec![]);
        let payment = WriteObj::new_received(payment, payee_id);
        self.insert(payment, debit_note_ids, invoice_ids).await
    }

    pub async fn get(&self, payment_id: String, owner_id: NodeId) -> DbResult<Option<Payment>> {
        readonly_transaction(self.pool, move |conn| {
            let payment: Option<ReadObj> = query!()
                .filter(dsl::id.eq(&payment_id))
                .filter(dsl::owner_id.eq(&owner_id))
                .first(conn)
                .optional()?;

            match payment {
                Some(payment) => {
                    let debit_note_ids = debit_note_dsl::pay_payment_x_debit_note
                        .select(debit_note_dsl::debit_note_id)
                        .filter(debit_note_dsl::payment_id.eq(&payment_id))
                        .filter(debit_note_dsl::owner_id.eq(&owner_id))
                        .load(conn)?;
                    let invoice_ids = invoice_dsl::pay_payment_x_invoice
                        .select(invoice_dsl::invoice_id)
                        .filter(invoice_dsl::payment_id.eq(&payment_id))
                        .filter(invoice_dsl::owner_id.eq(&owner_id))
                        .load(conn)?;
                    Ok(Some(payment.into_api_model(debit_note_ids, invoice_ids)))
                }
                None => Ok(None),
            }
        })
        .await
    }

    async fn get_for_role(
        &self,
        node_id: NodeId,
        later_than: Option<NaiveDateTime>,
        role: Role,
    ) -> DbResult<Vec<Payment>> {
        readonly_transaction(self.pool, move |conn| {
            let query = query!()
                .filter(dsl::owner_id.eq(&node_id))
                .filter(dsl::role.eq(&role))
                .order_by(dsl::timestamp.asc());
            let payments: Vec<ReadObj> = match later_than {
                Some(timestamp) => query.filter(dsl::timestamp.gt(timestamp)).load(conn)?,
                None => query.load(conn)?,
            };
            let debit_notes = debit_note_dsl::pay_payment_x_debit_note
                .inner_join(
                    dsl::pay_payment.on(debit_note_dsl::owner_id
                        .eq(dsl::owner_id)
                        .and(debit_note_dsl::payment_id.eq(dsl::id))),
                )
                .filter(dsl::owner_id.eq(&node_id))
                .filter(dsl::role.eq(&role))
                .select(crate::schema::pay_payment_x_debit_note::all_columns)
                .load(conn)?;
            let invoices = invoice_dsl::pay_payment_x_invoice
                .inner_join(
                    dsl::pay_payment.on(invoice_dsl::owner_id
                        .eq(dsl::owner_id)
                        .and(invoice_dsl::payment_id.eq(dsl::id))),
                )
                .filter(dsl::owner_id.eq(&node_id))
                .filter(dsl::role.eq(&role))
                .select(crate::schema::pay_payment_x_invoice::all_columns)
                .load(conn)?;
            Ok(join_payments_with_debit_notes_and_invoices(
                payments,
                debit_notes,
                invoices,
            ))
        })
        .await
    }

    pub async fn get_for_requestor(
        &self,
        node_id: NodeId,
        later_than: Option<NaiveDateTime>,
    ) -> DbResult<Vec<Payment>> {
        self.get_for_role(node_id, later_than, Role::Requestor)
            .await
    }

    pub async fn get_for_provider(
        &self,
        node_id: NodeId,
        later_than: Option<NaiveDateTime>,
    ) -> DbResult<Vec<Payment>> {
        self.get_for_role(node_id, later_than, Role::Provider).await
    }
}

fn join_payments_with_debit_notes_and_invoices(
    payments: Vec<ReadObj>,
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
        .map(|payment| {
            let debit_note_ids = debit_notes_map.remove(&payment.id).unwrap_or(vec![]);
            let invoice_ids = invoices_map.remove(&payment.id).unwrap_or(vec![]);
            payment.into_api_model(debit_note_ids, invoice_ids)
        })
        .collect()
}
