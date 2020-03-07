use crate::error::DbResult;
use crate::models::*;
use crate::schema::pay_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_invoice::dsl;
use crate::schema::pay_invoice_x_activity::dsl as activity_dsl;
use bigdecimal::BigDecimal;
use diesel::sql_types::Text;
use diesel::QueryableByName;
use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use std::collections::HashMap;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::payment::local::StatusNotes;
use ya_persistence::executor::{do_with_transaction, AsDao, ConnType, PoolType};
use ya_persistence::types::{BigDecimalField, Summable};

pub struct InvoiceDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for InvoiceDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> InvoiceDao<'c> {
    pub async fn create(&self, invoice: NewInvoice) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let activity_ids = invoice.activity_ids;
            let mut invoice = invoice.invoice;
            let invoice_id = invoice.id.clone();
            // TODO: Move last_debit_note_id assignment to database trigger
            invoice.last_debit_note_id = debit_note_dsl::pay_debit_note
                .select(debit_note_dsl::id)
                .filter(debit_note_dsl::agreement_id.eq(invoice.agreement_id.clone()))
                .order_by(debit_note_dsl::timestamp.desc())
                .first(conn)
                .optional()?;
            diesel::insert_into(dsl::pay_invoice)
                .values(invoice)
                .execute(conn)?;
            // Diesel cannot do batch insert into SQLite database
            activity_ids.into_iter().try_for_each(|activity_id| {
                let invoice_id = invoice_id.clone();
                diesel::insert_into(activity_dsl::pay_invoice_x_activity)
                    .values(InvoiceXActivity {
                        invoice_id,
                        activity_id,
                    })
                    .execute(conn)
                    .map(|_| ())
            })?;

            Ok(())
        })
        .await
    }

    pub async fn insert(&self, invoice: Invoice) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            // TODO: Check last_debit_note_id
            let activity_ids = invoice.activity_ids;
            let invoice_id = invoice.invoice.id.clone();
            diesel::insert_into(dsl::pay_invoice)
                .values(invoice.invoice)
                .execute(conn)?;
            // Diesel cannot do batch insert into SQLite database
            activity_ids.into_iter().try_for_each(|activity_id| {
                let invoice_id = invoice_id.clone();
                diesel::insert_into(activity_dsl::pay_invoice_x_activity)
                    .values(InvoiceXActivity {
                        invoice_id,
                        activity_id,
                    })
                    .execute(conn)
                    .map(|_| ())
            })?;
            Ok(())
        })
        .await
    }

    pub async fn get(&self, invoice_id: String) -> DbResult<Option<Invoice>> {
        do_with_transaction(self.pool, move |conn| {
            let invoice: Option<BareInvoice> = dsl::pay_invoice
                .find(invoice_id.clone())
                .first(conn)
                .optional()?;
            match invoice {
                Some(invoice) => {
                    let activity_ids = activity_dsl::pay_invoice_x_activity
                        .select(activity_dsl::activity_id)
                        .filter(activity_dsl::invoice_id.eq(invoice_id))
                        .load(conn)?;
                    Ok(Some(Invoice {
                        invoice,
                        activity_ids,
                    }))
                }
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn get_all(&self) -> DbResult<Vec<Invoice>> {
        do_with_transaction(self.pool, move |conn| {
            let invoices = dsl::pay_invoice.load(conn)?;
            let activities = activity_dsl::pay_invoice_x_activity.load(conn)?;
            Ok(join_invoices_with_activities(invoices, activities))
        })
        .await
    }

    pub async fn get_issued(&self, issuer_id: String) -> DbResult<Vec<Invoice>> {
        do_with_transaction(self.pool, move |conn| {
            let invoices = dsl::pay_invoice
                .filter(dsl::issuer_id.eq(issuer_id.clone()))
                .load(conn)?;
            let activities = activity_dsl::pay_invoice_x_activity
                .inner_join(dsl::pay_invoice)
                .filter(dsl::issuer_id.eq(issuer_id))
                .select(crate::schema::pay_invoice_x_activity::all_columns)
                .load(conn)?;
            Ok(join_invoices_with_activities(invoices, activities))
        })
        .await
    }

    pub async fn get_received(&self, recipient_id: String) -> DbResult<Vec<Invoice>> {
        do_with_transaction(self.pool, move |conn| {
            let invoices = dsl::pay_invoice
                .filter(dsl::recipient_id.eq(recipient_id.clone()))
                .load(conn)?;
            let activities = activity_dsl::pay_invoice_x_activity
                .inner_join(dsl::pay_invoice)
                .filter(dsl::recipient_id.eq(recipient_id))
                .select(crate::schema::pay_invoice_x_activity::all_columns)
                .load(conn)?;
            Ok(join_invoices_with_activities(invoices, activities))
        })
        .await
    }

    pub async fn update_status(&self, invoice_id: String, status: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::pay_invoice.find(invoice_id))
                .set(dsl::status.eq(status))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_total_amount(&self, invoice_ids: Vec<String>) -> DbResult<BigDecimal> {
        do_with_transaction(self.pool, move |conn| {
            let amounts: Vec<BigDecimalField> = dsl::pay_invoice
                .filter(dsl::id.eq_any(invoice_ids))
                .select(dsl::amount)
                .load(conn)?;
            Ok(amounts.sum())
        })
        .await
    }

    pub async fn get_accounts_ids(&self, invoice_ids: Vec<String>) -> DbResult<Vec<String>> {
        do_with_transaction(self.pool, move |conn| {
            let account_ids: Vec<String> = dsl::pay_invoice
                .filter(dsl::id.eq_any(invoice_ids))
                .select(dsl::credit_account_id)
                .distinct()
                .load(conn)?;
            Ok(account_ids)
        })
        .await
    }

    pub async fn status_report(&self, identity: NodeId) -> DbResult<(StatusNotes, StatusNotes)> {
        #[derive(QueryableByName, Default)]
        struct SettledAmount {
            #[sql_type = "Text"]
            total_amount_due: BigDecimalField,
        }

        fn find_settled_amount(
            c: &ConnType,
            identity: NodeId,
            agreement_id: &str,
        ) -> diesel::QueryResult<BigDecimal> {
            let f = diesel::sql_query(
                r#"
            SELECT total_amount_due
                FROM pay_debit_note as n
            WHERE status = 'SETTLED'
            AND (issuer_id = ? or recipient_id=?)
            AND agreement_id = ?
            AND NOT EXISTS (SELECT 1 FROM pay_debit_note
            where previous_debit_note_id = n.id and status = 'SETTLED')
            "#,
            )
            .bind::<Text, _>(identity)
            .bind::<Text, _>(identity)
            .bind::<Text, _>(agreement_id)
            .get_result::<SettledAmount>(c)
            .optional()
            .map_err(|e| {
                log::error!("get pay_debit_note for invoice: {}", e);
                e
            })?;

            Ok(f.unwrap_or_default().total_amount_due.into())
        }

        do_with_transaction(self.pool, move |c| {
            let invoices: Vec<BareInvoice> = diesel::sql_query(
                r#"
                    SELECT *
                    FROM pay_invoice
                    WHERE status in ('RECEIVED', 'ACCEPTED','REJECTED')
                    AND issuer_id = ? or recipient_id = ?"#,
            )
            .bind(identity)
            .bind(identity)
            .get_results(c)
            .map_err(|e| {
                log::error!("get BareInvoice: {}", e);
                e
            })?;
            let mut incoming = StatusNotes::default();
            let mut outgoing = StatusNotes::default();
            let me = identity.to_string();
            for invoice in invoices {
                let s = if invoice.issuer_id == me {
                    &mut incoming
                } else {
                    &mut outgoing
                };
                let settled = find_settled_amount(c, identity, invoice.agreement_id.as_str())?;

                let pending_amount = invoice.amount.0 - settled;
                match invoice.status.as_str() {
                    "RECEIVED" => s.requested += pending_amount,
                    "ACCEPTED" => s.accepted += pending_amount,
                    "REJECTED" => s.rejected += pending_amount,
                    _ => (),
                }
            }
            Ok((incoming, outgoing))
        })
        .await
    }
}

fn join_invoices_with_activities(
    invoices: Vec<BareInvoice>,
    activities: Vec<InvoiceXActivity>,
) -> Vec<Invoice> {
    let mut activities_map = activities
        .into_iter()
        .fold(HashMap::new(), |mut map, activity| {
            map.entry(activity.invoice_id)
                .or_insert_with(Vec::new)
                .push(activity.activity_id);
            map
        });
    invoices
        .into_iter()
        .map(|invoice| Invoice {
            activity_ids: activities_map.remove(&invoice.id).unwrap_or(vec![]),
            invoice,
        })
        .collect()
}
