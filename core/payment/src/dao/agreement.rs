use crate::dao::{invoice, invoice_event};
use crate::error::{DbError, DbResult};
use crate::models::agreement::{ReadObj, WriteObj};
use crate::schema::pay_activity::dsl as activity_dsl;
use crate::schema::pay_agreement::dsl;
use crate::schema::pay_invoice::dsl as invoice_dsl;
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use std::error::Error;
use std::fmt;
use std::str::FromStr;
use ya_client_model::market::Agreement;
use ya_client_model::payment::{DocumentStatus, InvoiceEventType};
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
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let total_amount_due = &agreement.total_amount_due + amount;
    diesel::update(&agreement)
        .set(dsl::total_amount_due.eq(total_amount_due))
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
    diesel::update(&agreement)
        .set(dsl::total_amount_due.eq(total_amount_due))
        .execute(conn)?;
    Ok(())
}

/// Compute and set amount due based on agreements
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
    diesel::update(&agreement)
        .set(dsl::total_amount_due.eq(total_amount_due))
        .execute(conn)?;
    Ok(())
}

pub fn increase_amount_accepted(
    agreement_id: &String,
    owner_id: &NodeId,
    amount: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let total_amount_accepted = &agreement.total_amount_accepted + amount;
    diesel::update(&agreement)
        .set(dsl::total_amount_accepted.eq(total_amount_accepted))
        .execute(conn)?;
    Ok(())
}

pub fn increase_amount_scheduled(
    agreement_id: &String,
    owner_id: &NodeId,
    amount: &BigDecimal,
    conn: &ConnType,
) -> DbResult<()> {
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let total_amount_scheduled: BigDecimalField =
        (&agreement.total_amount_scheduled.0 + amount).into();
    diesel::update(&agreement)
        .set(dsl::total_amount_scheduled.eq(total_amount_scheduled))
        .execute(conn)?;
    Ok(())
}

pub fn set_amount_accepted(
    agreement_id: &String,
    owner_id: NodeId,
    total_amount_accepted: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    diesel::update(&agreement)
        .set(dsl::total_amount_accepted.eq(total_amount_accepted))
        .execute(conn)?;
    Ok(())
}

#[derive(Debug)]
struct IncreaseAmountPaidError {
    details: String,
}
impl IncreaseAmountPaidError {
    fn new(msg: String) -> IncreaseAmountPaidError {
        IncreaseAmountPaidError { details: msg }
    }
}
impl fmt::Display for IncreaseAmountPaidError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}
impl Error for IncreaseAmountPaidError {
    fn description(&self) -> &str {
        &self.details
    }
}

pub fn increase_amount_paid(
    agreement_id: &String,
    owner_id: NodeId,
    amount: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    let total_amount_paid: BigDecimalField = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .select(dsl::total_amount_paid)
        .first(conn)?;
    let total_amount_paid = &total_amount_paid + amount;
    diesel::update(dsl::pay_agreement.find((agreement_id, owner_id)))
        .set(dsl::total_amount_paid.eq(&total_amount_paid))
        .execute(conn)?;

    //extract invoice for this activity
    //check if the total amount paid is equal to the total amount due
    //we cannot do that in sql due to lack of decimal support in sqlite
    let invoice_query: Option<(String, String, Role)> = invoice_dsl::pay_invoice
        .filter(invoice_dsl::agreement_id.eq(agreement_id))
        .filter(invoice_dsl::owner_id.eq(owner_id))
        .filter(invoice_dsl::status.ne_all(vec![
            DocumentStatus::Cancelled.to_string(),
            DocumentStatus::Settled.to_string(),
        ]))
        //.filter(invoice_dsl::amount.le(&total_amount_paid))
        .select((invoice_dsl::id, invoice_dsl::amount, invoice_dsl::role))
        .first(conn)
        .optional()?;

    if let Some((invoice_id, amount, role)) = invoice_query {
        let invoice_amount = BigDecimal::from_str(&amount)
            .map_err(|e| DbError::Query(format!("Failed to parse amount from invoice: {}", e)))?;

        if invoice_amount <= total_amount_paid.0 {
            invoice::update_status(&invoice_id, owner_id, DocumentStatus::Settled, conn)?;
            invoice_event::create(
                invoice_id,
                owner_id,
                InvoiceEventType::InvoiceSettledEvent,
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

impl AgreementDao<'_> {
    /*pub async fn list(&self, role: Option<Role>) -> DbResult<Vec<ReadObj>> {
        readonly_transaction(self.pool, "pay_agreement_dao_list", move |conn| {
            let agreements = dsl::pay_agreement.load(conn)?;
            Ok(agreements.into_iter().collect())
        })
        .await
    }*/

    pub async fn get(&self, agreement_id: String, owner_id: NodeId) -> DbResult<Option<ReadObj>> {
        readonly_transaction(self.pool, "pay_agreement_dao_get", move |conn| {
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
        do_with_transaction(self.pool, "agreement_dao_create_if", move |conn| {
            let existing: Option<String> = dsl::pay_agreement
                .find((&agreement.agreement_id, &owner_id))
                .select(dsl::id)
                .first(conn)
                .optional()?;
            if existing.is_some() {
                return Ok(());
            }

            let agreement = WriteObj::try_new(agreement, role).map_err(DbError::Query)?;
            diesel::insert_into(dsl::pay_agreement)
                .values(agreement)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_items: Option<u32>,
    ) -> DbResult<Vec<ReadObj>> {
        readonly_transaction(self.pool, "agreement_dao_get_for_node_id", move |conn| {
            let mut query = dsl::pay_agreement.into_boxed();
            query = query.filter(dsl::owner_id.eq(node_id));
            if let Some(date) = after_timestamp {
                query = query.filter(dsl::created_ts.gt(date))
            }
            query = query.order_by(dsl::created_ts.asc());
            if let Some(items) = max_items {
                query = query.limit(items.into())
            }
            let agreements: Vec<WriteObj> = query.load(conn)?;
            Ok(agreements)
        })
        .await
    }

    pub async fn get_transaction_balance(
        &self,
        node_id: NodeId,
        payee_addr: String,
        payer_addr: String,
    ) -> DbResult<BigDecimal> {
        readonly_transaction(
            self.pool,
            "agreement_dao_get_transaction_balance",
            move |conn| {
                let balance = dsl::pay_agreement
                    .select(dsl::total_amount_paid)
                    .filter(dsl::owner_id.eq(node_id))
                    .filter(dsl::payee_addr.eq(payee_addr))
                    .filter(dsl::payer_addr.eq(payer_addr))
                    .get_results::<BigDecimalField>(conn)?
                    .sum();
                Ok(balance)
            },
        )
        .await
    }

    /// Get total requested/accepted/paid amount of incoming transactions
    pub async fn incoming_transaction_summary(
        &self,
        platform: String,
        payee_addr: String,
        after_timestamp: NaiveDateTime,
    ) -> DbResult<StatusNotes> {
        readonly_transaction(self.pool, "agreement_dao_incoming_summary", move |conn| {
            let agreements: Vec<ReadObj> = dsl::pay_agreement
                .filter(dsl::role.eq(Role::Provider))
                .filter(dsl::payment_platform.eq(platform))
                .filter(dsl::payee_addr.eq(payee_addr))
                .filter(diesel::dsl::exists(
                    invoice_dsl::pay_invoice
                        .filter(invoice_dsl::agreement_id.eq(dsl::id))
                        .filter(invoice_dsl::timestamp.gt(after_timestamp))
                        .limit(1)
                        .select(invoice_dsl::id),
                ))
                .select(crate::schema::pay_agreement::all_columns)
                .get_results(conn)?;
            Ok(make_summary(agreements))
        })
        .await
    }

    /// Get total requested/accepted/paid amount of outgoing transactions
    pub async fn outgoing_transaction_summary(
        &self,
        platform: String,
        payer_addr: String,
        after_timestamp: NaiveDateTime,
    ) -> DbResult<StatusNotes> {
        readonly_transaction(self.pool, "agreement_dao_outgoing_summary", move |conn| {
            let agreements: Vec<ReadObj> = dsl::pay_agreement
                .filter(dsl::role.eq(Role::Requestor))
                .filter(dsl::payment_platform.eq(platform))
                .filter(dsl::payer_addr.eq(payer_addr))
                .filter(diesel::dsl::exists(
                    invoice_dsl::pay_invoice
                        .filter(invoice_dsl::agreement_id.eq(dsl::id))
                        .filter(invoice_dsl::timestamp.gt(after_timestamp))
                        .limit(1)
                        .select(invoice_dsl::id),
                ))
                .select(crate::schema::pay_agreement::all_columns)
                .get_results(conn)?;
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
        })
        .sum()
}
