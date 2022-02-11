use std::collections::HashMap;

use bigdecimal::{BigDecimal, ToPrimitive, Zero};
use chrono::{DateTime, Utc};
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
use crate::schema::pay_batch_order_item::dsl as oidsl;

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
    let (payments, total_amount) =
        resolve_new_payments(conn, owner_id, payer_addr, platform, since)?;

    if total_amount.is_zero() {
        return Ok(None);
    }

    let order_id = Uuid::new_v4().to_string();
    {
        use crate::schema::pay_batch_order::dsl as odsl;

        let _ = diesel::insert_into(odsl::pay_batch_order)
            .values((
                odsl::id.eq(&order_id),
                odsl::owner_id.eq(owner_id),
                odsl::payer_addr.eq(&payer_addr),
                odsl::platform.eq(&platform),
                odsl::total_amount.eq(total_amount.to_f32()),
            ))
            .execute(conn)?;
    }
    {
        for (payee_addr, payment) in payments {
            diesel::insert_into(oidsl::pay_batch_order_item)
                .values((
                    oidsl::id.eq(&order_id),
                    oidsl::payee_addr.eq(&payee_addr),
                    oidsl::amount.eq(BigDecimalField(payment.amount.clone())),
                ))
                .execute(conn)?;

            for (payee_id, obligations) in payment.peer_obligation {
                for obligation in obligations.iter() {
                    increase_amount_scheduled(conn, &owner_id, &obligation)?;
                }

                let json = serde_json::to_string(&obligations)?;
                use crate::schema::pay_batch_order_item_payment::dsl;

                diesel::insert_into(dsl::pay_batch_order_item_payment)
                    .values((
                        dsl::id.eq(&order_id),
                        dsl::payee_addr.eq(&payee_addr),
                        dsl::payee_id.eq(payee_id),
                        dsl::json.eq(json),
                    ))
                    .execute(conn)?;
            }
        }
    }
    Ok(Some(order_id))
}

fn resolve_new_payments(
    conn: &ConnType,
    owner_id: NodeId,
    payer_addr: &str,
    platform: &str,
    since: DateTime<Utc>,
) -> DbResult<(HashMap<String, BatchPayment>, BigDecimal)> {
    use crate::schema::pay_agreement::dsl as pa;
    use crate::schema::pay_invoice::dsl as iv;

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
        .filter(iv::timestamp.gt(since.naive_utc()))
        .filter(iv::status.eq("ACCEPTED"))
        .select((
            pa::id,
            pa::peer_id,
            pa::payee_addr,
            pa::total_amount_accepted,
            pa::total_amount_scheduled,
            iv::id,
            iv::amount,
        ))
        .load::<(
            String,
            NodeId,
            String,
            BigDecimalField,
            BigDecimalField,
            String,
            BigDecimalField,
        )>(conn)?;
    log::info!("found [{}] invoices", invoices.len());

    let mut total_amount = BigDecimal::default();
    let zero = BigDecimal::from(0u32);
    let mut payments = HashMap::<String, BatchPayment>::new();
    for (
        agreement_id,
        peer_id,
        payee_addr,
        total_amount_accepted,
        total_amount_scheduled,
        invoice_id,
        invoice_amount,
    ) in invoices
    {
        let amount = total_amount_accepted.0 - total_amount_scheduled.0;
        log::info!("[{}] to pay {} - {}", invoice_id, amount, agreement_id);

        if amount <= zero {
            super::invoice::update_status(&invoice_id, &owner_id, &DocumentStatus::Settled, conn)?;
            continue;
        }
        total_amount += &amount;

        let batch_payment = payments.entry(payee_addr.clone()).or_default();
        batch_payment.amount += &amount;

        let obligations = batch_payment.peer_obligation.entry(peer_id).or_default();
        obligations.push(BatchPaymentObligation::Invoice {
            id: invoice_id,
            amount,
            agreement_id: agreement_id.clone(),
        });
    }
    {
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
        }

        let v : Vec<Activity> = diesel::sql_query(r#"
                SELECT a.id, pa.peer_id, pa.payee_addr, a.total_amount_accepted, a.total_amount_scheduled, pa.id agreement_id
                FROM pay_activity a join pay_agreement pa on a.owner_id = pa.owner_id and a.agreement_id = pa.id and a.role = pa.role
                where a.role='R' and a.total_amount_accepted > 0
                and cast(a.total_amount_scheduled as float) < cast(a.total_amount_accepted as float)
                and not exists (select 1 from pay_invoice where agreement_id = a.agreement_id and owner_id = a.owner_id and role = 'R')
                and pa.updated_ts > ? and pa.payment_platform = ? and pa.owner_id = ?
            "#)
            .bind::<Timestamp, _>(since.naive_utc())
            .bind::<Text, _>(&platform)
            .bind::<Text, _>(owner_id)
            .load::<Activity>(conn)?;

        log::info!("{} activites found", v.len());

        for a in v {
            let amount = a.total_amount_accepted.0 - a.total_amount_scheduled.0;
            if amount < zero {
                continue;
            }
            total_amount += &amount;

            let batch_payment = payments.entry(a.payee_addr.clone()).or_default();
            batch_payment.amount += &amount;

            let obligations = batch_payment.peer_obligation.entry(a.peer_id).or_default();
            obligations.push(BatchPaymentObligation::DebitNote {
                amount,
                agreement_id: a.agreement_id.clone(),
                activity_id: a.id,
            });
        }
    }

    Ok((payments, total_amount))
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
            super::activity::increase_amount_scheduled(&activity_id, owner_id, amount, conn)
        }
    }
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
                .filter(oidsl::paid.eq(false))
                .load(conn)?;
            Ok((order, items))
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
                .filter(oidsl::id.eq(order_id).and(oidsl::payee_addr.eq(payee_addr)))
                .set(oidsl::driver_order_id.eq(driver_order_id))
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
                        .and(oidsl::paid.eq(false)),
                )
                .set(oidsl::paid.eq(true))
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
                        match obligation {
                            BatchPaymentObligation::Invoice {
                                id,
                                amount,
                                agreement_id,
                                ..
                            } => {
                                super::agreement::increase_amount_paid(
                                    &agreement_id,
                                    &order.owner_id,
                                    &BigDecimalField(amount),
                                    conn,
                                )?;
                            }
                            BatchPaymentObligation::DebitNote {
                                amount,
                                agreement_id,
                                activity_id,
                            } => {
                                super::activity::increase_amount_paid(
                                    &activity_id,
                                    &order.owner_id,
                                    &BigDecimalField(amount),
                                    conn,
                                )?;
                            }
                        }
                    }
                }
            }

            Ok(v)
        })
        .await
    }
}
