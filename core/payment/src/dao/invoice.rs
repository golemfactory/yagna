use crate::dao::{agreement, invoice_event};
use crate::error::{DbError, DbResult};
use crate::models::invoice::{equivalent, InvoiceXActivity, ReadObj, WriteObj};
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_invoice::dsl;
use crate::schema::pay_invoice_x_activity::dsl as activity_dsl;
use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::{
    BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl, RunQueryDsl,
};
use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use ya_client_model::payment::{DocumentStatus, Invoice, InvoiceEventType, NewInvoice};
use ya_client_model::NodeId;
use ya_core_model::payment::local::{DriverName, NetworkName, StatValue};
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{BigDecimalField, Role, Summable};

lazy_static::lazy_static! {
   static ref SQL_STR_WILDCARD: String = "%".to_string();
}

pub struct InvoiceDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for InvoiceDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

// FIXME: This could probably be a function
macro_rules! query {
    () => {
        dsl::pay_invoice
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
                dsl::status,
                dsl::timestamp,
                dsl::amount,
                dsl::payment_due_date,
                agreement_dsl::peer_id,
                agreement_dsl::payee_addr,
                agreement_dsl::payer_addr,
                agreement_dsl::payment_platform,
            ))
    };
}

pub fn update_status(
    invoice_id: &String,
    owner_id: &NodeId,
    status: &DocumentStatus,
    conn: &ConnType,
) -> DbResult<()> {
    diesel::update(
        dsl::pay_invoice
            .filter(dsl::id.eq(invoice_id))
            .filter(dsl::owner_id.eq(owner_id)),
    )
    .set(dsl::status.eq(status.to_string()))
    .execute(conn)?;
    Ok(())
}

impl<'c> InvoiceDao<'c> {
    async fn insert(&self, invoice: WriteObj, activity_ids: Vec<String>) -> DbResult<()> {
        let invoice_id = invoice.id.clone();
        let owner_id = invoice.owner_id.clone();
        let role = invoice.role.clone();
        do_with_transaction(self.pool, move |conn| {
            if let Some(read_invoice) = query!()
                .filter(dsl::id.eq(&invoice_id))
                .filter(dsl::owner_id.eq(owner_id))
                .first(conn)
                .optional()?
            {
                return match equivalent(&read_invoice, &invoice) {
                    true => Ok(()),
                    false => Err(DbError::Integrity(format!(
                        "Invoice with the same id and different content already exists."
                    ))),
                };
            };

            agreement::set_amount_due(&invoice.agreement_id, &owner_id, &invoice.amount, conn)?;

            diesel::insert_into(dsl::pay_invoice)
                .values(invoice)
                .execute(conn)?;

            // Diesel cannot do batch insert into SQLite database
            activity_ids.into_iter().try_for_each(|activity_id| {
                let invoice_id = invoice_id.clone();
                let owner_id = owner_id.clone();
                diesel::insert_into(activity_dsl::pay_invoice_x_activity)
                    .values(InvoiceXActivity {
                        invoice_id,
                        activity_id,
                        owner_id,
                    })
                    .execute(conn)
                    .map(|_| ())
            })?;

            if let Role::Requestor = role {
                invoice_event::create::<()>(
                    invoice_id,
                    owner_id,
                    InvoiceEventType::InvoiceReceivedEvent,
                    None,
                    conn,
                )?;
            }

            Ok(())
        })
        .await
    }

    pub async fn create_new(&self, invoice: NewInvoice, issuer_id: NodeId) -> DbResult<String> {
        let activity_ids = invoice.activity_ids.clone().unwrap_or(vec![]);
        let invoice = WriteObj::new_issued(invoice, issuer_id.clone());
        let invoice_id = invoice.id.clone();
        self.insert(invoice, activity_ids).await?;
        Ok(invoice_id)
    }

    pub async fn insert_received(&self, invoice: Invoice) -> DbResult<()> {
        let activity_ids = invoice.activity_ids.clone();
        let invoice = WriteObj::new_received(invoice);
        self.insert(invoice, activity_ids).await
    }

    pub async fn get(&self, invoice_id: String, owner_id: NodeId) -> DbResult<Option<Invoice>> {
        readonly_transaction(self.pool, move |conn| {
            let invoice: Option<ReadObj> = query!()
                .filter(dsl::id.eq(&invoice_id))
                .filter(dsl::owner_id.eq(owner_id))
                .first(conn)
                .optional()?;
            match invoice {
                Some(invoice) => {
                    let activity_ids = activity_dsl::pay_invoice_x_activity
                        .select(activity_dsl::activity_id)
                        .filter(activity_dsl::invoice_id.eq(invoice_id))
                        .filter(activity_dsl::owner_id.eq(owner_id))
                        .load(conn)?;
                    Ok(Some(invoice.into_api_model(activity_ids)?))
                }
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn get_many(
        &self,
        invoice_ids: Vec<String>,
        owner_id: NodeId,
    ) -> DbResult<Vec<Invoice>> {
        readonly_transaction(self.pool, move |conn| {
            let invoices = query!()
                .filter(dsl::id.eq_any(invoice_ids))
                .filter(dsl::owner_id.eq(owner_id))
                .load(conn)?;
            let activities = activity_dsl::pay_invoice_x_activity.load(conn)?;
            join_invoices_with_activities(invoices, activities)
        })
        .await
    }

    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_items: Option<u32>,
    ) -> DbResult<Vec<Invoice>> {
        readonly_transaction(self.pool, move |conn| {
            let mut query = query!().filter(dsl::owner_id.eq(node_id)).into_boxed();
            if let Some(date) = after_timestamp {
                query = query.filter(dsl::timestamp.gt(date))
            }
            if let Some(items) = max_items {
                query = query.limit(items.into())
            }
            let invoices = query.load(conn)?;
            let activities = activity_dsl::pay_invoice_x_activity
                .inner_join(
                    dsl::pay_invoice.on(activity_dsl::owner_id
                        .eq(dsl::owner_id)
                        .and(activity_dsl::invoice_id.eq(dsl::id))),
                )
                .filter(dsl::owner_id.eq(node_id))
                .select(crate::schema::pay_invoice_x_activity::all_columns)
                .load(conn)?;
            join_invoices_with_activities(invoices, activities)
        })
        .await
    }

    pub async fn last_invoice_stats(
        &self,
        node_id: NodeId,
        since: DateTime<Utc>,
        network: Option<NetworkName>,
        driver: Option<DriverName>,
    ) -> DbResult<BTreeMap<(Role, DocumentStatus), StatValue>> {
        let results = readonly_transaction(self.pool, move |conn| {
            let mut query = query!()
                .filter(dsl::owner_id.eq(node_id))
                .filter(dsl::timestamp.gt(since.naive_utc()));
            if network.is_some() || driver.is_some() {
                query = query
                    .filter(agreement_dsl::payment_platform
                        .like(format!(
                            "{}-{}%",
                            network.as_ref().unwrap_or(&*SQL_STR_WILDCARD),
                            driver.as_ref().unwrap_or(&*SQL_STR_WILDCARD));
                        );
                    );
            };
            let invoices: Vec<ReadObj> = query.load(conn)?;
            Ok::<_, DbError>(invoices)
        })
        .await?;
        let mut stats = BTreeMap::<(Role, DocumentStatus), StatValue>::new();
        for invoice in results {
            let key = (invoice.role, DocumentStatus::try_from(invoice.status)?);
            let entry = stats.entry(key).or_default();
            let total_amount = invoice.amount.0;
            *entry = entry.clone()
                + StatValue {
                    total_amount,
                    agreements_count: 1,
                };
        }
        Ok(stats)
    }

    pub async fn mark_received(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            update_status(&invoice_id, &owner_id, &DocumentStatus::Received, conn)
        })
        .await
    }

    pub async fn accept(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let (agreement_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_invoice
                .find((&invoice_id, &owner_id))
                .select((dsl::agreement_id, dsl::amount, dsl::role))
                .first(conn)?;
            let mut events = vec![InvoiceEventType::InvoiceAcceptedEvent];

            // Zero-amount invoices should be settled immediately.
            let status = if amount.0 == BigDecimal::from(0) {
                events.push(InvoiceEventType::InvoiceSettledEvent);
                DocumentStatus::Settled
            } else {
                DocumentStatus::Accepted
            };

            update_status(&invoice_id, &owner_id, &status, conn)?;
            agreement::set_amount_accepted(&agreement_id, &owner_id, &amount, conn)?;
            if let Role::Provider = role {
                for event in events {
                    invoice_event::create::<()>(
                        invoice_id.clone(),
                        owner_id.clone(),
                        event,
                        None,
                        conn,
                    )?;
                }
            }

            Ok(())
        })
        .await
    }

    // TODO: Implement reject invoice
    // pub async fn reject(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
    //     do_with_transaction(self.pool, move |conn| {
    //         let (agreement_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_invoice
    //             .find((&invoice_id, &owner_id))
    //             .select((dsl::agreement_id, dsl::amount, dsl::role))
    //             .first(conn)?;
    //         update_status(&invoice_id, &owner_id, &DocumentStatus::Accepted, conn)?;
    //         if let Role::Provider = role {
    //             invoice_event::create::<()>(invoice_id, owner_id, InvoiceEventType::InvoiceRejectedEvent { ... }, None, conn)?;
    //         }
    //         Ok(())
    //     })
    //     .await
    // }

    pub async fn cancel(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let (agreement_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_invoice
                .find((&invoice_id, &owner_id))
                .select((dsl::agreement_id, dsl::amount, dsl::role))
                .first(conn)?;

            agreement::compute_amount_due(&agreement_id, &owner_id, conn)?;

            update_status(&invoice_id, &owner_id, &DocumentStatus::Cancelled, conn)?;
            if let Role::Requestor = role {
                invoice_event::create::<()>(
                    invoice_id,
                    owner_id,
                    InvoiceEventType::InvoiceCancelledEvent,
                    None,
                    conn,
                )?;
            }
            Ok(())
        })
        .await
    }

    pub async fn get_total_amount(
        &self,
        invoice_ids: Vec<String>,
        owner_id: NodeId,
    ) -> DbResult<BigDecimal> {
        readonly_transaction(self.pool, move |conn| {
            let amounts: Vec<BigDecimalField> = dsl::pay_invoice
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::id.eq_any(invoice_ids))
                .select(dsl::amount)
                .load(conn)?;
            Ok(amounts.sum())
        })
        .await
    }
}

fn join_invoices_with_activities(
    invoices: Vec<ReadObj>,
    activities: Vec<InvoiceXActivity>,
) -> DbResult<Vec<Invoice>> {
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
        .map(|invoice| {
            let activity_ids = activities_map.remove(&invoice.id).unwrap_or(vec![]);
            invoice.into_api_model(activity_ids)
        })
        .collect()
}
