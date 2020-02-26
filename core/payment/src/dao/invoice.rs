use crate::error::DbResult;
use crate::models::*;
use crate::schema::pay_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_invoice::dsl;
use crate::schema::pay_invoice_x_activity::dsl as activity_dsl;
use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use std::collections::HashMap;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

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
            let invoice: Option<PureInvoice> = dsl::pay_invoice
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
}

fn join_invoices_with_activities(
    invoices: Vec<PureInvoice>,
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
