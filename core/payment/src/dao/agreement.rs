use crate::dao::{invoice, invoice_event};
use crate::error::{DbError, DbResult};
use crate::models::agreement::{ReadObj, WriteObj};
use crate::schema::pay_activity::dsl as activity_dsl;
use crate::schema::pay_agreement::dsl;
use crate::schema::pay_invoice::dsl as invoice_dsl;
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use std::collections::HashMap;
use ya_client_model::market::Agreement;
use ya_client_model::payment::{DocumentStatus, Invoice, InvoiceEventType};
use ya_client_model::NodeId;
use ya_core_model::payment::local::{StatValue, StatusNotes};
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{BigDecimalField, Role, Summable};

pub fn increase_amount_due(
    agreement_id: &String,
    owner_id: &NodeId,
    amount: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    assert!(amount > &BigDecimal::zero().into()); // TODO: Remove when payment service is production-ready.
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let total_amount_due = &agreement.total_amount_due + amount;
    let updated_ts = chrono::Utc::now().naive_utc();
    diesel::update(&agreement)
        .set((
            dsl::total_amount_due.eq(total_amount_due),
            dsl::updated_ts.eq(updated_ts),
        ))
        .execute(conn)?;
    Ok(())
}

pub fn set_amount_due(
    agreement_id: &String,
    owner_id: &NodeId,
    total_amount_due: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    if total_amount_due < &agreement.total_amount_due {
        return Err(DbError::Query(format!("Requested amount for agreement cannot be lowered. Current amount requested: {} Amount on invoice: {}", agreement.total_amount_due, total_amount_due)));
    }
    let updated_ts = chrono::Utc::now().naive_utc();
    diesel::update(&agreement)
        .set((
            dsl::total_amount_due.eq(total_amount_due),
            dsl::updated_ts.eq(updated_ts),
        ))
        .execute(conn)?;
    Ok(())
}

/// Compute and set amount due based on activities
pub fn compute_amount_due(
    agreement_id: &String,
    owner_id: &NodeId,
    conn: &ConnType,
) -> DbResult<()> {
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let activity_amounts: Vec<BigDecimalField> = activity_dsl::pay_activity
        .filter(activity_dsl::owner_id.eq(owner_id))
        .filter(activity_dsl::agreement_id.eq(agreement_id))
        .select(activity_dsl::total_amount_due)
        .load(conn)?;
    let total_amount_due: BigDecimalField = activity_amounts.sum().into();
    let updated_ts = chrono::Utc::now().naive_utc();
    diesel::update(&agreement)
        .set((
            dsl::total_amount_due.eq(total_amount_due),
            dsl::updated_ts.eq(updated_ts),
        ))
        .execute(conn)?;
    Ok(())
}

pub fn increase_amount_accepted(
    agreement_id: &String,
    owner_id: &NodeId,
    amount: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    assert!(amount > &BigDecimal::zero().into()); // TODO: Remove when payment service is production-ready.
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let total_amount_accepted = &agreement.total_amount_accepted + amount;
    let updated_ts = chrono::Utc::now().naive_utc();
    diesel::update(&agreement)
        .set((
            dsl::total_amount_accepted.eq(total_amount_accepted),
            dsl::updated_ts.eq(updated_ts),
        ))
        .execute(conn)?;
    Ok(())
}

pub fn increase_amount_scheduled(
    agreement_id: &String,
    owner_id: &NodeId,
    amount: &BigDecimal,
    conn: &ConnType,
) -> DbResult<()> {
    assert!(amount > &BigDecimal::zero().into()); // TODO: Remove when payment service is production-ready.
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let total_amount_scheduled: BigDecimalField =
        (&agreement.total_amount_scheduled.0 + amount).into();
    let updated_ts = chrono::Utc::now().naive_utc();
    diesel::update(&agreement)
        .set((
            dsl::total_amount_scheduled.eq(total_amount_scheduled),
            dsl::updated_ts.eq(updated_ts),
        ))
        .execute(conn)?;
    Ok(())
}

pub fn set_amount_accepted(
    agreement_id: &String,
    owner_id: &NodeId,
    total_amount_accepted: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    assert!(total_amount_accepted >= &agreement.total_amount_accepted); // TODO: Remove when payment service is production-ready.
    let updated_ts = chrono::Utc::now().naive_utc();
    diesel::update(&agreement)
        .set((
            dsl::total_amount_accepted.eq(total_amount_accepted),
            dsl::updated_ts.eq(updated_ts),
        ))
        .execute(conn)?;
    Ok(())
}

pub fn increase_amount_paid(
    agreement_id: &String,
    owner_id: &NodeId,
    amount: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    assert!(amount > &BigDecimal::zero().into()); // TODO: Remove when payment service is production-ready.
    let total_amount_paid: BigDecimalField = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .select(dsl::total_amount_paid)
        .first(conn)?;
    let total_amount_paid = &total_amount_paid + amount;
    let updated_ts = chrono::Utc::now().naive_utc();
    diesel::update(dsl::pay_agreement.find((agreement_id, owner_id)))
        .set((
            dsl::total_amount_paid.eq(&total_amount_paid),
            dsl::updated_ts.eq(updated_ts),
        ))
        .execute(conn)?;

    let invoice_query: Option<(String, Role)> = invoice_dsl::pay_invoice
        .filter(invoice_dsl::agreement_id.eq(agreement_id))
        .filter(invoice_dsl::owner_id.eq(owner_id))
        .filter(invoice_dsl::status.ne_all(vec![
            DocumentStatus::Cancelled.to_string(),
            DocumentStatus::Settled.to_string(),
        ]))
        .filter(invoice_dsl::amount.le(&total_amount_paid))
        .select((invoice_dsl::id, invoice_dsl::role))
        .first(conn)
        .optional()?;

    if let Some((invoice_id, role)) = invoice_query {
        invoice::update_status(&invoice_id, owner_id, &DocumentStatus::Settled, conn)?;
        if let Role::Provider = role {
            invoice_event::create::<()>(
                invoice_id,
                owner_id.clone(),
                InvoiceEventType::InvoiceSettledEvent,
                None,
                conn,
            )?;
        }
    }

    Ok(())
}

pub struct AgreementDao<'a> {
    pool: &'a PoolType,
}

impl<'a> AsDao<'a> for AgreementDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        Self { pool }
    }
}

impl<'a> AgreementDao<'a> {
    pub async fn get(&self, agreement_id: String, owner_id: NodeId) -> DbResult<Option<ReadObj>> {
        readonly_transaction(self.pool, move |conn| {
            let agreement = dsl::pay_agreement
                .find((agreement_id, owner_id))
                .first(conn)
                .optional()?;
            Ok(agreement)
        })
        .await
    }

    pub async fn create_if_not_exists(
        &self,
        agreement: Agreement,
        owner_id: NodeId,
        role: Role,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            let existing: Option<String> = dsl::pay_agreement
                .find((&agreement.agreement_id, &owner_id))
                .select(dsl::id)
                .first(conn)
                .optional()?;
            if let Some(_) = existing {
                return Ok(());
            }

            let agreement = WriteObj::new(agreement, role);
            diesel::insert_into(dsl::pay_agreement)
                .values(agreement)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_transaction_balance(
        &self,
        node_id: NodeId,
        payee_addr: String,
        payer_addr: String,
    ) -> DbResult<BigDecimal> {
        readonly_transaction(self.pool, move |conn| {
            let balance = dsl::pay_agreement
                .select(dsl::total_amount_paid)
                .filter(dsl::owner_id.eq(node_id))
                .filter(dsl::payee_addr.eq(payee_addr))
                .filter(dsl::payer_addr.eq(payer_addr))
                .get_results::<BigDecimalField>(conn)?
                .sum();
            Ok(balance)
        })
        .await
    }

    /// Get total requested/accepted/paid amount of incoming transactions
    pub async fn incoming_transaction_summary(
        &self,
        platform: String,
        payee_addr: String,
        since: Option<DateTime<Utc>>,
    ) -> DbResult<StatusNotes> {
        readonly_transaction(self.pool, move |conn| {
            let last_days = chrono::Utc::now() - chrono::Duration::days(1);
            let invoices: Vec<Invoice> =
                crate::dao::invoice::get_for_payee(conn, &payee_addr, Some(last_days.naive_utc()))?;

            let query = dsl::pay_agreement
                .filter(dsl::role.eq(Role::Provider))
                .filter(dsl::payment_platform.eq(platform))
                .filter(dsl::payee_addr.eq(payee_addr));

            let agreements: Vec<crate::models::agreement::ReadObj> = if let Some(last_days) = since
            {
                query
                    .filter(dsl::created_ts.ge(last_days.naive_utc()))
                    .get_results(conn)?
            } else {
                query.get_results(conn)?
            };

            Ok(make_summary2(agreements, invoices))
        })
        .await
    }

    /// Get total requested/accepted/paid amount of outgoing transactions
    pub async fn outgoing_transaction_summary(
        &self,
        platform: String,
        payer_addr: String,
        since: Option<DateTime<Utc>>,
    ) -> DbResult<StatusNotes> {
        readonly_transaction(self.pool, move |conn| {
            let query = dsl::pay_agreement
                .filter(dsl::role.eq(Role::Requestor))
                .filter(dsl::payment_platform.eq(platform))
                .filter(dsl::payer_addr.eq(payer_addr));
            let agreements: Vec<ReadObj> = if let Some(since) = since {
                query
                    .filter(dsl::created_ts.ge(since.naive_utc()))
                    .get_results(conn)?
            } else {
                query.get_results(conn)?
            };
            Ok(make_summary(agreements))
        })
        .await
    }
}

fn make_summary(agreements: Vec<ReadObj>) -> StatusNotes {
    agreements
        .into_iter()
        .map(|agreement| StatusNotes {
            requested: StatValue::new(agreement.total_amount_due),
            accepted: StatValue::new(agreement.total_amount_accepted),
            confirmed: StatValue::new(agreement.total_amount_paid),
            overdue: None,
        })
        .sum()
}

fn make_summary2(
    agreements: Vec<crate::models::agreement::ReadObj>,
    invoices: Vec<Invoice>,
) -> StatusNotes {
    let invoice_map: HashMap<String, Invoice> = invoices
        .into_iter()
        .map(|invoice| (invoice.agreement_id.clone(), invoice))
        .collect();

    fn is_overdue(
        agreement: &crate::models::agreement::ReadObj,
        invoice: Option<&Invoice>,
    ) -> bool {
        if let Some(invoice) = invoice {
            match invoice.status {
                DocumentStatus::Settled => false,
                DocumentStatus::Accepted => {
                    chrono::Utc::now() - invoice.payment_due_date > chrono::Duration::hours(1)
                }
                _ => false,
            }
        } else {
            false
        }
    }

    agreements
        .into_iter()
        .map(|agreement| {
            let is_overdue = is_overdue(&agreement, invoice_map.get(&agreement.id));
            let confirmed = agreement.total_amount_paid;
            let requested = if is_overdue {
                confirmed.clone()
            } else {
                agreement.total_amount_due
            };
            let accepted = if is_overdue {
                confirmed.clone()
            } else {
                agreement.total_amount_accepted.clone()
            };
            let overdue = if is_overdue {
                agreement.total_amount_accepted
            } else {
                Default::default()
            };
            StatusNotes {
                requested: StatValue::new(requested),
                accepted: StatValue::new(accepted),
                confirmed: StatValue::new(confirmed),
                overdue: Some(StatValue::new(overdue)),
            }
        })
        .sum()
}
