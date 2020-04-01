use crate::error::DbResult;
use crate::models::*;
use crate::schema::pay_invoice::dsl as invoice_dsl;
use crate::schema::pay_invoice_event::dsl;
use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

pub struct InvoiceEventDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for InvoiceEventDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> InvoiceEventDao<'c> {
    pub async fn create(&self, event: NewInvoiceEvent) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::pay_invoice_event)
                .values(event)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_for_recipient(
        &self,
        recipient_id: String,
        later_than: Option<NaiveDateTime>,
    ) -> DbResult<Vec<InvoiceEvent>> {
        do_with_transaction(self.pool, move |conn| {
            let query = dsl::pay_invoice_event
                .inner_join(invoice_dsl::pay_invoice)
                .filter(invoice_dsl::recipient_id.eq(recipient_id))
                .select(crate::schema::pay_invoice_event::all_columns)
                .order_by(dsl::timestamp.asc());
            let events = match later_than {
                Some(timestamp) => query.filter(dsl::timestamp.gt(timestamp)).load(conn)?,
                None => query.load(conn)?,
            };
            Ok(events)
        })
        .await
    }

    pub async fn get_for_issuer(
        &self,
        issuer_id: String,
        later_than: Option<NaiveDateTime>,
    ) -> DbResult<Vec<InvoiceEvent>> {
        do_with_transaction(self.pool, move |conn| {
            let query = dsl::pay_invoice_event
                .inner_join(invoice_dsl::pay_invoice)
                .filter(invoice_dsl::issuer_id.eq(issuer_id))
                .select(crate::schema::pay_invoice_event::all_columns)
                .order_by(dsl::timestamp.asc());
            let events = match later_than {
                Some(timestamp) => query.filter(dsl::timestamp.gt(timestamp)).load(conn)?,
                None => query.load(conn)?,
            };
            Ok(events)
        })
        .await
    }
}
