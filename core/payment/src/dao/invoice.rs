use crate::dao::{agreement, invoice_event};
use crate::error::DbResult;
use crate::models::invoice::{InvoiceXActivity, ReadObj, WriteObj};
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_invoice::dsl;
use crate::schema::pay_invoice_x_activity::dsl as activity_dsl;
use bigdecimal::BigDecimal;
use diesel::{
    BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl, RunQueryDsl,
};
use std::collections::HashMap;
use ya_client_model::payment::{EventType, Invoice, InvoiceStatus, NewInvoice};
use ya_core_model::ethaddr::NodeId;
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
            ))
    };
}

pub fn update_status(
    invoice_ids: &Vec<String>,
    owner_id: &NodeId,
    status: &InvoiceStatus,
    conn: &ConnType,
) -> DbResult<()> {
    diesel::update(
        dsl::pay_invoice
            .filter(dsl::id.eq_any(invoice_ids))
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
                invoice_event::create::<()>(invoice_id, owner_id, EventType::Received, None, conn)?;
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
                    Ok(Some(invoice.into_api_model(activity_ids)))
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
            Ok(join_invoices_with_activities(invoices, activities))
        })
        .await
    }

    async fn get_for_role(&self, node_id: NodeId, role: Role) -> DbResult<Vec<Invoice>> {
        readonly_transaction(self.pool, move |conn| {
            let invoices = query!()
                .filter(dsl::owner_id.eq(node_id))
                .filter(dsl::role.eq(&role))
                .load(conn)?;
            let activities = activity_dsl::pay_invoice_x_activity
                .inner_join(
                    dsl::pay_invoice.on(activity_dsl::owner_id
                        .eq(dsl::owner_id)
                        .and(activity_dsl::invoice_id.eq(dsl::id))),
                )
                .filter(dsl::owner_id.eq(node_id))
                .filter(dsl::role.eq(role))
                .select(crate::schema::pay_invoice_x_activity::all_columns)
                .load(conn)?;
            Ok(join_invoices_with_activities(invoices, activities))
        })
        .await
    }

    pub async fn get_for_provider(&self, node_id: NodeId) -> DbResult<Vec<Invoice>> {
        self.get_for_role(node_id, Role::Provider).await
    }

    pub async fn get_for_requestor(&self, node_id: NodeId) -> DbResult<Vec<Invoice>> {
        self.get_for_role(node_id, Role::Requestor).await
    }

    pub async fn mark_received(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            update_status(&vec![invoice_id], &owner_id, &InvoiceStatus::Received, conn)?;
            Ok(())
        })
        .await
    }

    pub async fn mark_failed(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            update_status(&vec![invoice_id], &owner_id, &InvoiceStatus::Failed, conn)?;
            Ok(())
        })
        .await
    }

    pub async fn accept(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let (agreement_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_invoice
                .find((&invoice_id, &owner_id))
                .select((dsl::agreement_id, dsl::amount, dsl::role))
                .first(conn)?;
            update_status(
                &vec![invoice_id.clone()],
                &owner_id,
                &InvoiceStatus::Accepted,
                conn,
            )?;
            agreement::set_amount_accepted(&agreement_id, &owner_id, &amount, conn)?;
            if let Role::Provider = role {
                invoice_event::create::<()>(invoice_id, owner_id, EventType::Accepted, None, conn)?;
            }
            Ok(())
        })
        .await
    }

    pub async fn reject(&self, invoice_id: String, owner_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let (agreement_id, amount, role): (String, BigDecimalField, Role) = dsl::pay_invoice
                .find((&invoice_id, &owner_id))
                .select((dsl::agreement_id, dsl::amount, dsl::role))
                .first(conn)?;
            update_status(
                &vec![invoice_id.clone()],
                &owner_id,
                &InvoiceStatus::Accepted,
                conn,
            )?;
            if let Role::Provider = role {
                invoice_event::create::<()>(invoice_id, owner_id, EventType::Rejected, None, conn)?;
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
        .map(|invoice| {
            let activity_ids = activities_map.remove(&invoice.id).unwrap_or(vec![]);
            invoice.into_api_model(activity_ids)
        })
        .collect()
}
