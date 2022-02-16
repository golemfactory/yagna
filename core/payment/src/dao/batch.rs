use std::collections::{HashMap, HashSet};

use bigdecimal::{BigDecimal, ToPrimitive, Zero};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Text, Timestamp};
use uuid::Uuid;

use ya_client_model::payment::DocumentStatus;
use ya_core_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::BigDecimalField;

use crate::error::{DbError, DbResult};
use crate::models::batch::*;
use crate::schema::pay_batch_order::dsl;
use crate::schema::pay_batch_order_item::dsl as oidsl;
use crate::schema::pay_batch_order_item_payment::dsl as pdsl;
use crate::schema::pay_debit_note::dsl as dndsl;
use crate::schema::pay_order::dsl as odsl;

table! {
    sql_activity (id, owner_id) {
        id -> Text,
        owner_id -> Text,
        role -> Text,
        peer_id -> Text,
        payee_addr -> Text,
        agreement_id -> Text,
        total_amount_due -> Text,
        total_amount_accepted -> Text,
        total_amount_scheduled -> Text,
        total_amount_paid -> Text,
        updated_ts -> Nullable<Timestamp>,
    }
}

#[derive(QueryableByName)]
#[table_name = "sql_activity"]
struct Activity {
    id: String,
    peer_id: NodeId,
    payee_addr: String,
    total_amount_accepted: BigDecimalField,
    total_amount_scheduled: BigDecimalField,
    agreement_id: String,
    updated_ts: Option<NaiveDateTime>,
}

pub struct BatchDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for BatchDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

pub fn resolve_invoices(
    conn: &ConnType,
    owner_id: NodeId,
    payer_addr: &str,
    platform: &str,
    since: DateTime<Utc>,
) -> DbResult<Option<String>> {
    let (payments, total_amount, _) =
        collect_pending_payments(conn, owner_id, payer_addr, platform, since)?;
    if total_amount.is_zero() {
        return Ok(None);
    }
    create_order(conn, owner_id, payer_addr, platform, payments, total_amount).map(Some)
}

pub fn batch_payments(
    conn: &ConnType,
    owner_id: NodeId,
    payer_addr: &str,
    platform: &str,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> DbResult<(HashSet<String>, DateTime<Utc>)> {
    let mut orders = Default::default();
    let (payments, total_amount, latest_entry) =
        collect_pending_payments(conn, owner_id, payer_addr, platform, since)?;

    if total_amount.is_zero() {
        return Ok((orders, Utc.from_utc_datetime(&latest_entry)));
    }

    log::debug!("Collected {} pending payments", payments.len());

    for (payee_addr, mut payment) in payments {
        let existing: Option<(
            String,
            Option<f32>,
            BigDecimalField,
            Option<NaiveDateTime>,
            NodeId,
            String,
        )> = oidsl::pay_batch_order_item
            .inner_join(dsl::pay_batch_order)
            .inner_join(
                pdsl::pay_batch_order_item_payment.on(pdsl::id
                    .eq(dsl::id)
                    .and(pdsl::payee_addr.eq(oidsl::payee_addr))),
            )
            .filter(oidsl::status.eq(DbBatchOrderItemStatus::Pending))
            .filter(oidsl::payee_addr.eq(&payee_addr))
            .filter(oidsl::payment_due_date.nullable().ge(now.naive_utc()))
            .filter(dsl::owner_id.eq(owner_id))
            .filter(dsl::payer_addr.eq(payer_addr))
            .filter(dsl::platform.eq(platform))
            .select((
                dsl::id,
                dsl::total_amount,
                oidsl::amount,
                oidsl::payment_due_date,
                pdsl::payee_id,
                pdsl::json,
            ))
            .order_by(oidsl::payment_due_date.desc())
            .first(conn)
            .optional()?;

        let sub_amount = payment.amount.clone();

        match existing {
            Some((order_id, total_amount, payment_amount, payment_due_date, payee_id, json)) => {
                log::debug!(
                    "Adding a payment of {} to {} ({}) to an existing batch: {}",
                    sub_amount,
                    payee_addr,
                    platform,
                    order_id
                );

                let mut vec: Vec<BatchPaymentObligation> = serde_json::from_str(&json)?;
                let obligations = payment.peer_obligation.entry(payee_id).or_default();
                vec.extend(obligations.drain(..));
                let json = serde_json::to_string(&vec)?;

                let current_due_date = payment.payment_due_dates.get(&payee_addr).cloned();
                let payment_due_date = payment_due_date
                    .min(current_due_date)
                    .or(payment_due_date)
                    .or(current_due_date);

                let total_amount = total_amount.unwrap_or(0.)
                    + sub_amount.to_f32().ok_or_else(|| {
                        DbError::Integrity(format!("Invalid amount: {}", sub_amount))
                    })?;

                diesel::update(dsl::pay_batch_order)
                    .filter(dsl::id.eq(&order_id))
                    .set(dsl::total_amount.eq(total_amount))
                    .execute(conn)?;

                diesel::update(oidsl::pay_batch_order_item)
                    .filter(oidsl::id.eq(&order_id))
                    .set((
                        oidsl::amount.eq(BigDecimalField(payment_amount.0 + sub_amount)),
                        oidsl::payment_due_date.eq(payment_due_date),
                    ))
                    .execute(conn)?;

                diesel::update(pdsl::pay_batch_order_item_payment)
                    .filter(pdsl::id.eq(&order_id))
                    .set(pdsl::json.eq(json))
                    .execute(conn)?;

                orders.insert(order_id);
            }
            None => {
                log::debug!(
                    "Adding a payment of {} to {} ({}) to a new batch",
                    sub_amount,
                    payee_addr,
                    platform,
                );

                let sub_payments = HashMap::from([(payee_addr.to_string(), payment)]);
                let order_id = create_order(
                    conn,
                    owner_id,
                    payer_addr,
                    platform,
                    sub_payments,
                    sub_amount,
                )?;

                orders.insert(order_id);
            }
        }
    }

    Ok((orders, Utc.from_utc_datetime(&latest_entry)))
}

pub fn get_batch_orders(
    conn: &ConnType,
    ids: &[String],
    platform: &str,
) -> DbResult<Vec<DbBatchOrderItem>> {
    let batch_orders: Vec<DbBatchOrderItem> = oidsl::pay_batch_order_item
        .filter(oidsl::driver_order_id.eq_any(ids))
        .load(conn)?;

    Ok(batch_orders)
}

fn create_order(
    conn: &ConnType,
    owner_id: NodeId,
    payer_addr: &str,
    platform: &str,
    payments: HashMap<String, BatchPayment>,
    total_amount: BigDecimal,
) -> DbResult<String> {
    let order_id = Uuid::new_v4().to_string();

    let _ = diesel::insert_into(dsl::pay_batch_order)
        .values((
            dsl::id.eq(&order_id),
            dsl::owner_id.eq(owner_id),
            dsl::payer_addr.eq(&payer_addr),
            dsl::platform.eq(&platform),
            dsl::total_amount.eq(total_amount.to_f32()),
        ))
        .execute(conn)?;

    for (payee_addr, payment) in payments {
        diesel::insert_into(oidsl::pay_batch_order_item)
            .values((
                oidsl::id.eq(&order_id),
                oidsl::payee_addr.eq(&payee_addr),
                oidsl::amount.eq(BigDecimalField(payment.amount.clone())),
                oidsl::payment_due_date.eq(payment.payment_due_dates.get(&payee_addr)),
            ))
            .execute(conn)?;

        for (payee_id, obligations) in payment.peer_obligation {
            let json = serde_json::to_string(&obligations)?;
            diesel::insert_into(pdsl::pay_batch_order_item_payment)
                .values((
                    pdsl::id.eq(&order_id),
                    pdsl::payee_addr.eq(&payee_addr),
                    pdsl::payee_id.eq(payee_id),
                    pdsl::json.eq(json),
                ))
                .execute(conn)?;
        }
    }

    Ok(order_id)
}

fn collect_pending_payments(
    conn: &ConnType,
    owner_id: NodeId,
    payer_addr: &str,
    platform: &str,
    since: DateTime<Utc>,
) -> DbResult<(HashMap<String, BatchPayment>, BigDecimal, NaiveDateTime)> {
    use crate::schema::pay_agreement::dsl as pa;
    use crate::schema::pay_invoice::dsl as iv;

    let since = since.naive_utc();
    let invoices = iv::pay_invoice
        .inner_join(
            pa::pay_agreement.on(pa::id
                .eq(iv::agreement_id)
                .and(pa::owner_id.eq(iv::owner_id))),
        )
        .filter(iv::owner_id.eq(owner_id))
        .filter(iv::role.eq("R"))
        .filter(pa::payer_addr.eq(payer_addr))
        .filter(pa::payment_platform.eq(platform))
        .filter(iv::timestamp.gt(since))
        .filter(iv::status.eq("ACCEPTED"))
        .select((
            pa::id,
            pa::peer_id,
            pa::payee_addr,
            pa::total_amount_accepted,
            pa::total_amount_scheduled,
            iv::id,
            iv::amount,
            iv::timestamp,
            iv::payment_due_date,
        ))
        .load::<(
            String,
            NodeId,
            String,
            BigDecimalField,
            BigDecimalField,
            String,
            BigDecimalField,
            NaiveDateTime,
            NaiveDateTime,
        )>(conn)?;
    log::info!("{} invoices found", invoices.len());

    let zero = BigDecimal::from(0u32);
    let mut payments = HashMap::<String, BatchPayment>::new();
    let mut total_amount = BigDecimal::default();
    let mut latest_entry = since;

    for (
        agreement_id,
        peer_id,
        payee_addr,
        total_amount_accepted,
        total_amount_scheduled,
        invoice_id,
        invoice_amount,
        invoice_timestamp,
        invoice_payment_due_date,
    ) in invoices
    {
        let amount = total_amount_accepted.0 - total_amount_scheduled.0;
        log::info!("[{}] to pay {} - {}", invoice_id, amount, agreement_id);
        if amount <= zero {
            super::invoice::update_status(&invoice_id, &owner_id, &DocumentStatus::Settled, conn)?;
            continue;
        }

        total_amount += &amount;
        let batch = payments.entry(payee_addr.clone()).or_default();
        batch.amount += &amount;

        let obligations = batch.peer_obligation.entry(peer_id).or_default();
        let obligation = BatchPaymentObligation::Invoice {
            id: invoice_id,
            amount,
            agreement_id: agreement_id.clone(),
        };
        increase_amount_scheduled(conn, &owner_id, &obligation)?;
        obligations.push(obligation);

        let current_due_date = batch.payment_due_dates.get(&payee_addr).cloned();
        let payment_due_date = current_due_date
            .min(Some(invoice_payment_due_date))
            .unwrap_or(invoice_payment_due_date);
        batch
            .payment_due_dates
            .insert(payee_addr.to_string(), payment_due_date);

        latest_entry = latest_entry.max(invoice_timestamp);
    }
    {
        let v : Vec<Activity> = diesel::sql_query(r#"
                SELECT a.id, pa.peer_id, pa.payee_addr, a.total_amount_accepted, a.total_amount_scheduled, pa.id agreement_id, pa.updated_ts
                FROM pay_activity a join pay_agreement pa on a.owner_id = pa.owner_id and a.agreement_id = pa.id and a.role = pa.role
                where a.role='R' and a.total_amount_accepted > 0
                and cast(a.total_amount_scheduled as float) < cast(a.total_amount_accepted as float)
                and not exists (select 1 from pay_invoice where agreement_id = a.agreement_id and owner_id = a.owner_id and role = 'R')
                and pa.updated_ts > ? and pa.payment_platform = ? and pa.owner_id = ?
            "#)
            .bind::<Timestamp, _>(since)
            .bind::<Text, _>(&platform)
            .bind::<Text, _>(&owner_id)
            .load::<Activity>(conn)?;

        log::info!("{} activites found", v.len());

        for a in v {
            let amount = a.total_amount_accepted.0 - a.total_amount_scheduled.0;
            if amount < zero {
                continue;
            }

            total_amount += &amount;
            let batch = payments.entry(a.payee_addr.clone()).or_default();
            batch.amount += &amount;

            let obligations = batch.peer_obligation.entry(a.peer_id).or_default();
            let obligation = BatchPaymentObligation::DebitNote {
                amount,
                agreement_id: a.agreement_id.clone(),
                activity_id: a.id.clone(),
            };
            increase_amount_scheduled(conn, &owner_id, &obligation)?;
            obligations.push(obligation);

            let dn_due_date: Option<NaiveDateTime> = dndsl::pay_debit_note
                .filter(dndsl::owner_id.eq(&owner_id))
                .filter(dndsl::activity_id.eq(&a.id))
                .filter(dndsl::role.eq("R"))
                .filter(dndsl::timestamp.gt(since))
                .filter(dndsl::status.eq_any(vec!["RECEIVED", "ACCEPTED"]))
                .filter(diesel::dsl::not(diesel::dsl::exists(
                    odsl::pay_order
                        .filter(odsl::debit_note_id.eq(dndsl::id.nullable()))
                        .filter(odsl::payee_addr.eq(&a.payee_addr))
                        .select(odsl::id),
                )))
                .select(dndsl::payment_due_date)
                .order_by(dndsl::payment_due_date.asc())
                .first(conn)
                .optional()?
                .flatten();

            let current_due_date = batch.payment_due_dates.get(&a.payee_addr).cloned();
            let payment_due_date = current_due_date
                .min(dn_due_date)
                .or(current_due_date)
                .or(dn_due_date);
            if let Some(due_date) = payment_due_date {
                batch
                    .payment_due_dates
                    .insert(a.payee_addr.to_string(), due_date);
            }

            latest_entry = a
                .updated_ts
                .map(|d| latest_entry.max(d))
                .unwrap_or(latest_entry);
        }
    }

    Ok((payments, total_amount, latest_entry))
}

fn increase_amount_scheduled(
    conn: &ConnType,
    owner_id: &NodeId,
    obligation: &BatchPaymentObligation,
) -> DbResult<()> {
    match obligation {
        BatchPaymentObligation::Invoice {
            id,
            amount,
            agreement_id,
        } => {
            log::debug!(
                "increase_amount_scheduled agreement_id={} by {}",
                agreement_id,
                amount
            );
            super::agreement::increase_amount_scheduled(agreement_id, owner_id, amount, conn)
        }
        BatchPaymentObligation::DebitNote {
            amount,
            agreement_id,
            activity_id,
        } => {
            log::debug!(
                "increase_amount_scheduled activity_id={} agreement_id={} by {}",
                activity_id,
                agreement_id,
                amount
            );
            super::activity::increase_amount_scheduled(activity_id, owner_id, amount, conn)
        }
    }
}

fn increase_amount_paid(
    conn: &ConnType,
    owner_id: &NodeId,
    obligation: &BatchPaymentObligation,
) -> DbResult<()> {
    match obligation {
        BatchPaymentObligation::Invoice {
            id,
            amount,
            agreement_id,
        } => {
            log::debug!(
                "increase_amount_paid agreement_id={} by {}",
                agreement_id,
                amount
            );
            let amount = BigDecimalField(amount.clone());
            super::agreement::increase_amount_paid(agreement_id, owner_id, &amount, conn)
        }
        BatchPaymentObligation::DebitNote {
            amount,
            agreement_id,
            activity_id,
        } => {
            log::debug!(
                "increase_amount_paid activity_id={} agreement_id={} by {}",
                activity_id,
                agreement_id,
                amount
            );
            let amount = BigDecimalField(amount.clone());
            super::activity::increase_amount_paid(activity_id, owner_id, &amount, conn)
        }
    }
}

impl<'c> BatchDao<'c> {
    pub async fn resolve(
        &self,
        owner_id: NodeId,
        payer_addr: String,
        platform: String,
        since: DateTime<Utc>,
    ) -> DbResult<Option<String>> {
        do_with_transaction(self.pool, move |conn| {
            resolve_invoices(conn, owner_id, &payer_addr, &platform, since)
        })
        .await
    }

    pub async fn batch(
        &self,
        owner_id: NodeId,
        payer_addr: String,
        platform: String,
        since: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> DbResult<(HashSet<String>, DateTime<Utc>)> {
        do_with_transaction(self.pool, move |conn| {
            batch_payments(conn, owner_id, &payer_addr, &platform, since, now)
        })
        .await
    }

    pub async fn list_debit_notes(
        &self,
        owner_id: NodeId,
        payment_platform: String,
        since: DateTime<Utc>,
    ) -> DbResult<Vec<(String, BigDecimalField, BigDecimalField)>> {
        use crate::schema::pay_activity;

        #[derive(QueryableByName)]
        #[table_name = "pay_activity"]
        struct Activity {
            id: String,
            total_amount_accepted: BigDecimalField,
            total_amount_scheduled: BigDecimalField,
        }

        do_with_transaction(self.pool, move |conn| {
            let v : Vec<Activity> = diesel::sql_query(r#"
                SELECT a.id, a.total_amount_accepted, a.total_amount_scheduled
                 FROM pay_activity a join pay_agreement pa on a.owner_id = pa.owner_id and a.agreement_id = pa.id and a.role = pa.role
                where a.role='R' and a.total_amount_accepted > 0
                and cast(a.total_amount_scheduled as float) < cast(a.total_amount_accepted as float)
                and not exists (select 1 from pay_invoice where agreement_id = a.agreement_id and owner_id = a.owner_id and role = 'R')
                and pa.updated_ts > ? and pa.payment_platform = ? and pa.owner_id = ?
            "#)
                .bind::<Timestamp, _>(since.naive_utc())
                .bind::<Text, _>(&payment_platform)
                .bind::<Text, _>(owner_id)
                .load::<Activity>(conn)?;
            Ok(v.into_iter().map(|a| (a.id, a.total_amount_accepted, a.total_amount_scheduled)).collect())
        }).await
    }

    pub async fn get_batch_order(&self, order_id: String) -> DbResult<DbBatchOrder> {
        readonly_transaction(self.pool, move |conn| {
            use crate::schema::pay_batch_order::dsl as odsl;

            Ok(odsl::pay_batch_order
                .filter(odsl::id.eq(order_id))
                .get_result(conn)?)
        })
        .await
    }

    pub async fn get_batch_order_payments(
        &self,
        order_id: String,
        payee_addr: String,
    ) -> DbResult<BatchPayment> {
        readonly_transaction(self.pool, |conn| {
            use crate::schema::pay_batch_order_item::dsl as di;
            use crate::schema::pay_batch_order_item_payment::dsl as d;

            let (amount,) = di::pay_batch_order_item
                .filter(di::id.eq(&order_id).and(di::payee_addr.eq(&payee_addr)))
                .select((di::amount,))
                .get_result::<(BigDecimalField,)>(conn)?;

            let mut peer_obligation = HashMap::<NodeId, Vec<BatchPaymentObligation>>::new();

            for (payee_id, json) in d::pay_batch_order_item_payment
                .filter(d::id.eq(order_id).and(d::payee_addr.eq(payee_addr)))
                .select((d::payee_id, d::json))
                .load::<(NodeId, String)>(conn)?
            {
                let obligations =
                    serde_json::from_str(&json).map_err(|e| DbError::Integrity(e.to_string()))?;
                peer_obligation.insert(payee_id, obligations);
            }

            Ok(BatchPayment {
                amount: amount.0,
                peer_obligation,
                payment_due_dates: Default::default(),
            })
        })
        .await
    }

    pub async fn get_unsent_batch_items(
        &self,
        order_id: String,
    ) -> DbResult<(DbBatchOrder, Vec<DbBatchOrderItem>)> {
        readonly_transaction(self.pool, move |conn| {
            use crate::schema::pay_batch_order::dsl as odsl;

            let order: DbBatchOrder = odsl::pay_batch_order
                .filter(odsl::id.eq(&order_id))
                .get_result(conn)?;
            let items: Vec<DbBatchOrderItem> = oidsl::pay_batch_order_item
                .filter(oidsl::id.eq(&order_id))
                .filter(oidsl::driver_order_id.is_null())
                .filter(oidsl::status.eq(DbBatchOrderItemStatus::Pending))
                .load(conn)?;
            Ok((order, items))
        })
        .await
    }

    pub async fn get_unsent_batch_orders(
        &self,
        due_date: Option<DateTime<Utc>>,
    ) -> DbResult<Vec<(DbBatchOrderItem, DbBatchOrder)>> {
        readonly_transaction(self.pool, move |conn| {
            let mut query = oidsl::pay_batch_order_item
                .inner_join(dsl::pay_batch_order)
                .filter(oidsl::status.eq(DbBatchOrderItemStatus::Pending))
                .into_boxed();

            if let Some(due_date) = due_date {
                query = query.filter(oidsl::payment_due_date.le(due_date.naive_utc()));
            }

            Ok(query.order_by(oidsl::payment_due_date.asc()).load(conn)?)
        })
        .await
    }

    pub async fn get_next_unsent_due_date(
        &self,
        order_id: String,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> DbResult<Option<DateTime<Utc>>> {
        readonly_transaction(self.pool, move |conn| {
            Ok(oidsl::pay_batch_order_item
                .inner_join(dsl::pay_batch_order)
                .filter(oidsl::status.eq(DbBatchOrderItemStatus::Pending))
                .select(oidsl::payment_due_date)
                .order_by(oidsl::payment_due_date.asc())
                .first(conn)
                .optional()?
                .flatten()
                .map(|d| Utc.from_utc_datetime(&d)))
        })
        .await
    }

    pub async fn batch_order_item_send(
        &self,
        order_id: String,
        payee_addr: String,
        driver_order_id: String,
    ) -> DbResult<usize> {
        do_with_transaction(self.pool, |conn| {
            Ok(diesel::update(oidsl::pay_batch_order_item)
                .filter(oidsl::id.eq(order_id))
                .filter(oidsl::payee_addr.eq(payee_addr))
                .set((
                    oidsl::driver_order_id.eq(driver_order_id),
                    oidsl::status.eq(DbBatchOrderItemStatus::Sent),
                ))
                .execute(conn)?)
        })
        .await
    }

    pub async fn batch_order_item_paid(
        &self,
        order_id: String,
        payee_addr: String,
        confirmation: Vec<u8>,
    ) -> DbResult<usize> {
        do_with_transaction(self.pool, move |conn| {
            use crate::schema::pay_batch_order::dsl as odsl;
            use crate::schema::pay_batch_order_item_payment::dsl as d;
            let order: DbBatchOrder = odsl::pay_batch_order
                .filter(odsl::id.eq(&order_id))
                .get_result(conn)?;

            let v = diesel::update(oidsl::pay_batch_order_item)
                .filter(
                    oidsl::id
                        .eq(&order_id)
                        .and(oidsl::payee_addr.eq(&payee_addr))
                        .and(oidsl::status.eq(DbBatchOrderItemStatus::Sent)),
                )
                .set(oidsl::status.eq(DbBatchOrderItemStatus::Paid))
                .execute(conn)?;
            if v > 0 {
                for (payee_id, json) in d::pay_batch_order_item_payment
                    .filter(d::id.eq(&order_id).and(d::payee_addr.eq(&payee_addr)))
                    .select((d::payee_id, d::json))
                    .load::<(NodeId, String)>(conn)?
                {
                    let obligations: Vec<BatchPaymentObligation> = serde_json::from_str(&json)
                        .map_err(|e| DbError::Integrity(e.to_string()))?;
                    for obligation in obligations {
                        increase_amount_paid(conn, &order.owner_id, &obligation)?;
                    }
                }
            }

            Ok(v)
        })
        .await
    }
}
