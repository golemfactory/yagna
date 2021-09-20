use crate::error::DbResult;
use bigdecimal::{BigDecimal, ToPrimitive};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Text, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;
use ya_client_model::payment::DocumentStatus;
use ya_core_model::NodeId;
use ya_persistence::executor::{do_with_transaction, AsDao, ConnType, PoolType};
use ya_persistence::types::BigDecimalField;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BatchPaymentObligation {
    Invoice {
        id: String,
        amount: BigDecimal,
        agreement_id: String,
    },
    DebitNote {
        id: String,
        amount: BigDecimal,
        agreement_id: String,
        activity_id: String,
    },
}

pub struct BatchItem {
    pub payee_addr: String,
    pub payments: Vec<BatchPayment>,
}

pub struct BatchPayment {
    pub amount: BigDecimal,
    pub peer_obligation: HashMap<NodeId, Vec<BatchPaymentObligation>>,
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
) -> DbResult<()> {
    todo!()
}

#[cfg(test)]
pub fn resolve_invoices(
    conn: &ConnType,
    owner_id: NodeId,
    payer_addr: &str,
    platform: &str,
    since: DateTime<Utc>,
) -> DbResult<()> {
    use crate::schema::pay_agreement::dsl as pa;
    use crate::schema::pay_invoice::dsl as iv;
    use std::collections::hash_map;

    let invoices = iv::pay_invoice
        .inner_join(pa::pay_agreement)
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

    let mut total_amount = BigDecimal::default();
    let zero = BigDecimal::default();
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
        let amount_to_pay = total_amount_scheduled.0 - total_amount_accepted.0;
        if amount_to_pay <= zero {
            super::invoice::update_status(&invoice_id, &owner_id, &DocumentStatus::Settled, conn)?;
            continue;
        }

        let obligation = BatchPaymentObligation::Invoice {
            id: invoice_id,
            amount: amount_to_pay.clone(),
            agreement_id: agreement_id,
        };

        match payments.entry(payee_addr.clone()) {
            hash_map::Entry::Occupied(mut e) => {
                let mut payment = e.get_mut();
                payment.amount += &amount_to_pay;
                match payment.peer_obligation.entry(peer_id) {
                    hash_map::Entry::Occupied(mut e) => e.get_mut().push(obligation),
                    hash_map::Entry::Vacant(e) => {
                        e.insert(vec![obligation]);
                    }
                }
            }
            hash_map::Entry::Vacant(e) => {
                let mut peer_obligation = HashMap::new();
                peer_obligation.insert(peer_id, vec![obligation]);
                let amount = amount_to_pay.clone();
                e.insert(BatchPayment {
                    amount,
                    peer_obligation,
                })
            }
        }
        super::agreement::increase_amount_scheduled(
            &agreement_id,
            &owner_id,
            &amount_to_pay,
            conn,
        )?;
    }
    {
        use crate::schema::pay_activity;
        use crate::schema::pay_activity::dsl;

        #[derive(QueryableByName)]
        #[table_name = "pay_activity"]
        struct Activity {
            id: String,
            peer_id: NodeId,
            payee_addr: String,
            total_amount_accepted: BigDecimalField,
            total_amount_scheduled: BigDecimalField,
        };

        let v : Vec<Activity> = diesel::sql_query(r#"
                SELECT a.id, pa.peer_id, pa.payee_addr, a.total_amount_accepted, a.total_amount_scheduled
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
        use crate::schema::pay_batch_order_item::dsl as oidsl;

        for (payee_addr, payment) in payments {
            diesel::insert_into(oidsl::pay_batch_order_item)
                .values((
                    oidsl::id.eq(&order_id),
                    oidsl::payee_addr.eq(&payee_addr),
                    oidsl::amount.eq(BigDecimalField(payment.amount.clone())),
                ))
                .execute(conn)?;
            for (payee_id, obligations) in payment.peer_obligation {
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
    Ok(())
}

impl<'c> BatchDao<'c> {
    pub async fn list_debit_notes(
        &self,
        owner_id: NodeId,
        payment_platform: String,
        since: DateTime<Utc>,
    ) -> DbResult<Vec<(String, BigDecimalField, BigDecimalField)>> {
        use crate::schema::pay_activity;
        use crate::schema::pay_debit_note::dsl;
        use crate::schema::pay_invoice_x_activity::dsl as ii;

        #[derive(QueryableByName)]
        #[table_name = "pay_activity"]
        struct Activity {
            id: String,
            total_amount_accepted: BigDecimalField,
            total_amount_scheduled: BigDecimalField,
        };

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

    pub async fn new_batch_order(
        &self,
        owner_id: NodeId,
        payer_addr: String,
        platform: String,
        items: Vec<BatchItem>,
    ) -> DbResult<String> {
        do_with_transaction(self.pool, move |conn| {
            let order_id = Uuid::new_v4().to_string();
            {
                use crate::schema::pay_batch_order::dsl;

                let total_amount: BigDecimal = Default::default(); //items.iter().map(|i| i.).sum();

                let v = diesel::insert_into(dsl::pay_batch_order)
                    .values((
                        dsl::id.eq(&order_id),
                        dsl::owner_id.eq(&owner_id),
                        dsl::payer_addr.eq(payer_addr),
                        dsl::platform.eq(platform),
                        dsl::total_amount.eq(total_amount.to_f32()),
                    ))
                    .execute(conn)?;
            }
            {
                use crate::schema::pay_batch_order_item::dsl;

                /*for (payee_addr, (amount, payments)) in items {
                    diesel::insert_into(dsl::pay_batch_order_item)
                        .values((
                            dsl::id.eq(&order_id),
                            dsl::payee_addr.eq(&payee_addr),
                            dsl::amount.eq(BigDecimalField(amount)),
                        ))
                        .execute(conn)?;
                    for (payee_id, json) in payments {
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
                }*/
                todo!();
            }

            Ok(order_id)
        })
        .await
    }
}
