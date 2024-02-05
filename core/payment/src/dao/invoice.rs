use crate::dao::{agreement, invoice_event};
use crate::error::{DbError, DbResult};
use crate::models::invoice::{equivalent, InvoiceXActivity, ReadObj, WriteObj};
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_invoice::dsl;
use crate::schema::pay_invoice_x_activity::dsl as activity_dsl;
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::{
    BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl, RunQueryDsl,
};
use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use ya_client_model::payment::{DocumentStatus, Invoice, InvoiceEventType, NewInvoice, Rejection};
use ya_client_model::NodeId;
use ya_core_model::payment::local::StatValue;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{BigDecimalField, Role, Summable};

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
        let owner_id = invoice.owner_id;
        let role = invoice.role.clone();
        do_with_transaction(self.pool, "invoice_dao_insert", move |conn| {
            if let Some(read_invoice) = query!()
                .filter(dsl::id.eq(&invoice_id))
                .filter(dsl::owner_id.eq(owner_id))
                .first(conn)
                .optional()?
            {
                return match equivalent(&read_invoice, &invoice) {
                    true => Ok(()),
                    false => Err(DbError::Integrity(
                        "Invoice with the same id and different content already exists."
                            .to_string(),
                    )),
                };
            };

            agreement::set_amount_due(&invoice.agreement_id, &owner_id, &invoice.amount, conn)?;

            diesel::insert_into(dsl::pay_invoice)
                .values(invoice)
                .execute(conn)?;

            // Diesel cannot do batch insert into SQLite database
            activity_ids.into_iter().try_for_each(|activity_id| {
                let invoice_id = invoice_id.clone();
                let invoice_owner_id = owner_id;
                diesel::insert_into(activity_dsl::pay_invoice_x_activity)
                    .values(InvoiceXActivity {
                        invoice_id,
                        activity_id,
                        owner_id: invoice_owner_id,
                    })
                    .execute(conn)
                    .map(|_| ())
            })?;

            invoice_event::create(
                invoice_id,
                owner_id,
                InvoiceEventType::InvoiceReceivedEvent,
                conn,
            )?;

            Ok(())
        })
        .await
    }

    pub async fn create_new(&self, invoice: NewInvoice, issuer_id: NodeId) -> DbResult<String> {
        let activity_ids = invoice.activity_ids.clone().unwrap_or_default();
        let invoice = WriteObj::new_issued(invoice, issuer_id);
        let invoice_id = invoice.id.clone();
        self.insert(invoice, activity_ids).await?;
        Ok(invoice_id)
    }

    pub async fn insert_received(&self, invoice: Invoice) -> DbResult<()> {
        let activity_ids = invoice.activity_ids.clone();
        let invoice = WriteObj::new_received(invoice);
        self.insert(invoice, activity_ids).await
    }

    pub async fn list(
        &self,
        role: Option<Role>,
        status: Option<DocumentStatus>,
    ) -> DbResult<Vec<Invoice>> {
        readonly_transaction(self.pool, "invoice_dao_list", move |conn| {
            let mut query = query!().into_boxed();
            if let Some(role) = role {
                query = query.filter(dsl::role.eq(role.to_string()));
            }
            if let Some(status) = status {
                query = query.filter(dsl::status.eq(status.to_string()));
            }

            let read_objs: Vec<ReadObj> = query.order_by(dsl::timestamp.desc()).load(conn)?;
            let mut invoices = Vec::<Invoice>::new();

            for read_obj in read_objs {
                let activity_ids = activity_dsl::pay_invoice_x_activity
                    .select(activity_dsl::activity_id)
                    .filter(activity_dsl::invoice_id.eq(&read_obj.id))
                    .filter(activity_dsl::owner_id.eq(read_obj.owner_id))
                    .load(conn)?;
                invoices.push(read_obj.into_api_model(activity_ids)?);
            }

            Ok(invoices)
        })
        .await
    }

    pub async fn get(&self, invoice_id: String, owner_id: NodeId) -> DbResult<Option<Invoice>> {
        readonly_transaction(self.pool, "invoice_dao_get", move |conn| {
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

    /*
     * Get invoice by agreement id
     * Only one invoice per agreement is allowed (it is enforced by the unique constraint on pay_invoice_x_activity)
     */
    pub async fn get_by_agreement(
        &self,
        agreement_id: String,
        owner_id: NodeId,
    ) -> DbResult<Option<Invoice>> {
        readonly_transaction(self.pool, "invoice_dao_get_by_agreement", move |conn| {
            let invoice: Option<ReadObj> = query!()
                .filter(dsl::agreement_id.eq(&agreement_id))
                .filter(dsl::owner_id.eq(owner_id))
                .first(conn)
                .optional()?;
            match invoice {
                Some(invoice) => {
                    let activity_ids = activity_dsl::pay_invoice_x_activity
                        .select(activity_dsl::activity_id)
                        .filter(activity_dsl::invoice_id.eq(invoice.id.clone()))
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
        readonly_transaction(self.pool, "invoice_dao_get_many", move |conn| {
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
        readonly_transaction(self.pool, "invoice_dao_get_for_node_id", move |conn| {
            let mut query = query!().filter(dsl::owner_id.eq(node_id)).into_boxed();
            if let Some(date) = after_timestamp {
                query = query.filter(dsl::timestamp.gt(date))
            }
            if let Some(items) = max_items {
                query = query.limit(items.into())
            }
            let invoices = query.order_by(dsl::timestamp.asc()).load(conn)?;
            let activities = activity_dsl::pay_invoice_x_activity
                .inner_join(
                    dsl::pay_invoice.on(activity_dsl::owner_id
                        .eq(dsl::owner_id)
                        .and(activity_dsl::invoice_id.eq(dsl::id))),
                )
                .filter(dsl::owner_id.eq(node_id))
                .order_by(dsl::timestamp.asc())
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
    ) -> DbResult<BTreeMap<(Role, DocumentStatus), StatValue>> {
        let results =
            readonly_transaction(self.pool, "invoice_dao_last_invoice_stats", move |conn| {
                let invoices: Vec<ReadObj> = query!()
                    .filter(dsl::owner_id.eq(node_id))
                    .filter(dsl::timestamp.gt(since.naive_utc()))
                    .load(conn)?;
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
        do_with_transaction(self.pool, "invoice_dao_mark_received", move |conn| {
            update_status(&invoice_id, &owner_id, &DocumentStatus::Received, conn)
        })
        .await
    }

    pub async fn accept(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, "invoice_dao_accept", move |conn| {
            let (agreement_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_invoice
                .find((&invoice_id, &owner_id))
                .select((dsl::agreement_id, dsl::amount, dsl::role))
                .first(conn)?;
            let mut events = vec![InvoiceEventType::InvoiceAcceptedEvent];

            // Zero-amount invoices should be settled immediately.
            let status = if amount.0.is_zero() {
                events.push(InvoiceEventType::InvoiceSettledEvent);
                DocumentStatus::Settled
            } else {
                DocumentStatus::Accepted
            };

            // Accept called on provider is invoked by the requestor, meaning the status must by synchronized.
            // For requestor, a separate `mark_accept_sent` call is required to mark synchronization when the information
            // is delivered to the Provider.
            if role == Role::Requestor {
                diesel::update(
                    dsl::pay_invoice
                        .filter(dsl::id.eq(invoice_id.clone()))
                        .filter(dsl::owner_id.eq(owner_id)),
                )
                .set(dsl::send_accept.eq(true))
                .execute(conn)?;
            }

            update_status(&invoice_id, &owner_id, &status, conn)?;
            agreement::set_amount_accepted(&agreement_id, &owner_id, &amount, conn)?;

            for event in events {
                invoice_event::create(invoice_id.clone(), owner_id, event, conn)?;
            }

            Ok(())
        })
        .await
    }

    /// Mark the invoice as synchronized with the other side.
    ///
    /// If the status in DB matches expected_status, `sync` is set to `true` and Ok(true) is returned.
    /// Otherwise, DB is not modified and Ok(false) is returned.
    ///
    /// Err(_) is only produced by DB issues.
    pub async fn mark_accept_sent(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, "invoice_dao_mark_accept_sent", move |conn| {
            diesel::update(
                dsl::pay_invoice
                    .filter(dsl::id.eq(invoice_id))
                    .filter(dsl::owner_id.eq(owner_id)),
            )
            .set(dsl::send_accept.eq(false))
            .execute(conn)?;
            Ok(())
        })
        .await
    }

    /// Lists invoices with send_accept
    pub async fn unsent_accepted(&self, owner_id: NodeId) -> DbResult<Vec<Invoice>> {
        readonly_transaction(self.pool, "invoice_dao_unsent_accepted", move |conn| {
            let invoices: Vec<ReadObj> = query!()
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::send_accept.eq(true))
                .filter(dsl::status.eq(DocumentStatus::Accepted.to_string()))
                .load(conn)?;

            let activities = activity_dsl::pay_invoice_x_activity
                .inner_join(
                    dsl::pay_invoice.on(activity_dsl::owner_id
                        .eq(dsl::owner_id)
                        .and(activity_dsl::invoice_id.eq(dsl::id))),
                )
                .filter(dsl::owner_id.eq(owner_id))
                .select(crate::schema::pay_invoice_x_activity::all_columns)
                .load(conn)?;
            join_invoices_with_activities(invoices, activities)
        })
        .await
    }

    /// All invoices with status Issued or Accepted and provider role
    pub async fn dangling(&self, owner_id: NodeId) -> DbResult<Vec<Invoice>> {
        readonly_transaction(self.pool, "invoice_dao_dangling", move |conn| {
            let invoices: Vec<ReadObj> = query!()
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::role.eq(Role::Provider.to_string()))
                .filter(
                    dsl::status
                        .eq(&DocumentStatus::Issued.to_string())
                        .or(dsl::status.eq(&DocumentStatus::Accepted.to_string())),
                )
                .load(conn)?;

            let activities = activity_dsl::pay_invoice_x_activity
                .inner_join(
                    dsl::pay_invoice.on(activity_dsl::owner_id
                        .eq(dsl::owner_id)
                        .and(activity_dsl::invoice_id.eq(dsl::id))),
                )
                .filter(dsl::owner_id.eq(owner_id))
                .select(crate::schema::pay_invoice_x_activity::all_columns)
                .load(conn)?;
            join_invoices_with_activities(invoices, activities)
        })
        .await
    }

    pub async fn reject(
        &self,
        invoice_id: String,
        owner_id: NodeId,
        rejection: Rejection,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, "invoice_reject", move |conn| {
            let (agreement_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_invoice
                .find((&invoice_id, &owner_id))
                .select((dsl::agreement_id, dsl::amount, dsl::role))
                .first(conn)?;
            update_status(&invoice_id, &owner_id, &DocumentStatus::Rejected, conn)?;
            if role == Role::Requestor {
                diesel::update(
                    dsl::pay_invoice
                        .filter(dsl::id.eq(invoice_id.clone()))
                        .filter(dsl::owner_id.eq(owner_id)),
                )
                .set(dsl::send_reject.eq(true))
                .execute(conn)?;
            }
            invoice_event::create(
                invoice_id,
                owner_id,
                InvoiceEventType::InvoiceRejectedEvent { rejection },
                conn,
            )?;
            Ok(())
        })
        .await
    }

    pub async fn mark_reject_sent(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, "mark_reject_sent", move |conn| {
            diesel::update(
                dsl::pay_invoice
                    .filter(dsl::id.eq(invoice_id))
                    .filter(dsl::owner_id.eq(owner_id)),
            )
            .set(dsl::send_reject.eq(false))
            .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn unsent_rejected(&self, owner_id: NodeId) -> DbResult<Vec<Invoice>> {
        readonly_transaction(self.pool, "unsent_rejected", move |conn| {
            let invoices: Vec<ReadObj> = query!()
                .filter(dsl::owner_id.eq(owner_id))
                .filter(dsl::send_reject.eq(true))
                .filter(dsl::status.eq(DocumentStatus::Rejected.to_string()))
                .load(conn)?;

            let activities = activity_dsl::pay_invoice_x_activity
                .inner_join(
                    dsl::pay_invoice.on(activity_dsl::owner_id
                        .eq(dsl::owner_id)
                        .and(activity_dsl::invoice_id.eq(dsl::id))),
                )
                .filter(dsl::owner_id.eq(owner_id))
                .select(crate::schema::pay_invoice_x_activity::all_columns)
                .load(conn)?;
            join_invoices_with_activities(invoices, activities)
        })
        .await
    }

    pub async fn cancel(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, "invoice_dao_cancel", move |conn| {
            let (agreement_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_invoice
                .find((&invoice_id, &owner_id))
                .select((dsl::agreement_id, dsl::amount, dsl::role))
                .first(conn)?;

            agreement::compute_amount_due(&agreement_id, &owner_id, conn)?;

            update_status(&invoice_id, &owner_id, &DocumentStatus::Cancelled, conn)?;
            invoice_event::create(
                invoice_id,
                owner_id,
                InvoiceEventType::InvoiceCancelledEvent,
                conn,
            )?;

            Ok(())
        })
        .await
    }

    pub async fn get_total_amount(
        &self,
        invoice_ids: Vec<String>,
        owner_id: NodeId,
    ) -> DbResult<BigDecimal> {
        readonly_transaction(self.pool, "invoice_dao_get_total_amount", move |conn| {
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

#[allow(clippy::unwrap_or_default)]
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
            let activity_ids = activities_map.remove(&invoice.id).unwrap_or_default();
            invoice.into_api_model(activity_ids)
        })
        .collect()
}
