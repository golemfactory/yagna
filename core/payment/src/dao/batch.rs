use std::collections::{hash_map, HashMap};

use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDateTime, Utc};
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

pub struct BatchDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for BatchDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

table! {
    sql_activity_join_agreement (id, owner_id) {
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
        debit_note_id -> Nullable<Text>,
    }
}

#[derive(QueryableByName)]
#[table_name = "sql_activity_join_agreement"]
struct ActivityJoinAgreement {
    id: String,
    peer_id: NodeId,
    payee_addr: String,
    total_amount_accepted: BigDecimalField,
    total_amount_scheduled: BigDecimalField,
    agreement_id: String,
    debit_note_id: Option<String>,
}

pub fn resolve_invoices_agreement_part(
    args: &ResolveInvoiceArgs,
    total_amount: BigDecimal,
    payments: HashMap<String, BatchPayment>,
) -> DbResult<(HashMap<String, BatchPayment>, BigDecimal)> {
    let conn = args.conn;
    let owner_id = args.owner_id;
    let payer_addr = args.payer_addr;
    let platform = args.platform;
    let since = args.since;
    let mut total_amount = total_amount;
    let mut payments = payments;
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

    let zero = BigDecimal::from(0u32);
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
        let amount_to_pay = total_amount_accepted.0 - total_amount_scheduled.0;
        log::info!(
            "[{}] to pay {} - {}",
            invoice_id,
            amount_to_pay,
            agreement_id
        );
        if amount_to_pay <= zero {
            super::invoice::update_status(&invoice_id, &owner_id, &DocumentStatus::Settled, conn)?;
            continue;
        }

        total_amount += &amount_to_pay;

        let obligation = BatchPaymentObligation::Invoice {
            id: invoice_id,
            amount: amount_to_pay.clone(),
            agreement_id: agreement_id.clone(),
        };

        match payments.entry(payee_addr.clone()) {
            hash_map::Entry::Occupied(mut e) => {
                let payment = e.get_mut();
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
                });
            }
        }
        log::debug!(
            "increase_amount_scheduled agreement_id={} by {}",
            agreement_id,
            amount_to_pay
        );
        super::agreement::increase_amount_scheduled(
            &agreement_id,
            &owner_id,
            &amount_to_pay,
            conn,
        )?;
    }
    Ok((payments, total_amount))
}

pub fn resolve_invoices_activity_part(
    args: &ResolveInvoiceArgs,
    total_amount: BigDecimal,
    payments: HashMap<String, BatchPayment>,
) -> DbResult<(HashMap<String, BatchPayment>, BigDecimal)> {
    let conn = args.conn;
    let owner_id = args.owner_id;
    let payer_addr = args.payer_addr;
    let platform = args.platform;
    let since = args.since;
    let mut total_amount = total_amount;
    let mut payments = payments;
    let zero = BigDecimal::from(0u32);

    {
        // query explanation
        // select all activities that are not fully paid
        // for each activity, find the last accepted debit note in debit note chain

        let query_res = diesel::sql_query(
            r#"
                SELECT a.id,
                    pa.peer_id,
                    pa.payee_addr,
                    a.total_amount_accepted,
                    a.total_amount_scheduled,
                    pa.id agreement_id,
                    (SELECT dn.id
                        FROM pay_debit_note dn
                        WHERE dn.activity_id = a.id
                            AND dn.owner_id = a.owner_id
                            AND dn.status = 'ACCEPTED'
                        ORDER BY dn.debit_nonce DESC
                        LIMIT 1
                    ) debit_note_id
                FROM pay_activity a JOIN pay_agreement pa
                    ON a.owner_id = pa.owner_id
                        AND a.agreement_id = pa.id
                        AND a.role = pa.role
                WHERE a.role='R'
                    AND a.total_amount_accepted != '0'
                    AND a.total_amount_scheduled != a.total_amount_accepted
                    AND pa.updated_ts > ?
                    AND pa.payment_platform = ?
                    AND pa.owner_id = ?
            "#,
        )
        .bind::<Timestamp, _>(since.naive_utc())
        .bind::<Text, _>(&platform)
        .bind::<Text, _>(owner_id)
        .load::<ActivityJoinAgreement>(conn)?;

        log::info!("Pay for activities - {} found to check", query_res.len());
        for a in query_res {
            let amount_to_pay =
                a.total_amount_accepted.0.clone() - a.total_amount_scheduled.0.clone();
            if amount_to_pay < zero {
                log::warn!("Activity {} has total_amount_scheduled: {} greater than total_amount_accepted: {}, which can be a bug",
                    a.id,
                    a.total_amount_scheduled.0.clone(),
                    a.total_amount_accepted.0.clone());
                continue;
            }
            total_amount += &amount_to_pay;
            super::activity::increase_amount_scheduled(&a.id, &owner_id, &amount_to_pay, conn)?;

            let obligation = BatchPaymentObligation::DebitNote {
                debit_note_id: a.debit_note_id,
                amount: amount_to_pay.clone(),
                agreement_id: a.agreement_id.clone(),
                activity_id: a.id,
            };

            match payments.entry(a.payee_addr.clone()) {
                hash_map::Entry::Occupied(mut e) => {
                    let payment = e.get_mut();
                    payment.amount += &amount_to_pay;
                    match payment.peer_obligation.entry(a.peer_id) {
                        hash_map::Entry::Occupied(mut e) => e.get_mut().push(obligation),
                        hash_map::Entry::Vacant(e) => {
                            e.insert(vec![obligation]);
                        }
                    }
                }
                hash_map::Entry::Vacant(e) => {
                    let mut peer_obligation = HashMap::new();
                    peer_obligation.insert(a.peer_id, vec![obligation]);
                    let amount = amount_to_pay.clone();
                    e.insert(BatchPayment {
                        amount,
                        peer_obligation,
                    });
                }
            }
        }
    }
    Ok((payments, total_amount))
}

pub struct ResolveInvoiceArgs<'a> {
    pub conn: &'a ConnType,
    pub owner_id: NodeId,
    pub payer_addr: &'a str,
    pub platform: &'a str,
    pub since: DateTime<Utc>,
}

pub fn resolve_invoices(args: &ResolveInvoiceArgs) -> DbResult<Option<String>> {
    let conn = args.conn;
    let owner_id = args.owner_id;
    let payer_addr = args.payer_addr;
    let platform = args.platform;
    let since = args.since;
    let zero = BigDecimal::from(0u32);

    let total_amount = BigDecimal::default();
    let payments = HashMap::<String, BatchPayment>::new();

    let total_amount = BigDecimal::from(0u32);

    log::info!("Resolve invoices: {} - {}", owner_id.to_string(), platform);

    log::debug!("Resolving invoices for {}", owner_id);
    let (payments, total_amount) = resolve_invoices_activity_part(args, total_amount, payments)?;

    log::debug!("Resolving agreements for {}", owner_id);
    let (payments, total_amount) = resolve_invoices_agreement_part(args, total_amount, payments)?;

    if total_amount == zero {
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
                odsl::total_amount.eq(total_amount.to_string()),
                odsl::paid_amount.eq("0"),
            ))
            .execute(conn)?;
    }
    {
        for (payee_addr, payment) in payments {
            diesel::insert_into(oidsl::pay_batch_order_item)
                .values((
                    oidsl::order_id.eq(&order_id),
                    oidsl::owner_id.eq(owner_id),
                    oidsl::payee_addr.eq(&payee_addr),
                    oidsl::amount.eq(BigDecimalField(payment.amount.clone())),
                ))
                .execute(conn)?;
            for (payee_id, obligations) in payment.peer_obligation {
                for obligation in &obligations {
                    log::debug!("obligation: {:?}", obligation);
                    match obligation {
                        BatchPaymentObligation::Invoice {
                            id,
                            amount,
                            agreement_id,
                        } => {
                            use crate::schema::pay_batch_order_item_document::dsl;
                            diesel::insert_into(dsl::pay_batch_order_item_document)
                                .values((
                                    dsl::order_id.eq(&order_id),
                                    dsl::owner_id.eq(owner_id),
                                    dsl::payee_addr.eq(&payee_addr),
                                    dsl::agreement_id.eq(agreement_id),
                                    dsl::invoice_id.eq(id),
                                    dsl::activity_id.eq(None::<String>),
                                    dsl::debit_note_id.eq(None::<String>),
                                    dsl::amount.eq(BigDecimalField(amount.clone())),
                                ))
                                .execute(conn)?;
                        }
                        BatchPaymentObligation::DebitNote {
                            amount,
                            debit_note_id,
                            agreement_id,
                            activity_id,
                        } => {
                            use crate::schema::pay_batch_order_item_document::dsl;
                            diesel::insert_into(dsl::pay_batch_order_item_document)
                                .values((
                                    dsl::order_id.eq(&order_id),
                                    dsl::owner_id.eq(owner_id),
                                    dsl::payee_addr.eq(&payee_addr),
                                    dsl::agreement_id.eq(agreement_id),
                                    dsl::invoice_id.eq(None::<String>),
                                    dsl::activity_id.eq(activity_id),
                                    dsl::debit_note_id.eq(debit_note_id),
                                    dsl::amount.eq(BigDecimalField(amount.clone())),
                                ))
                                .execute(conn)?;
                        }
                    }
                }
            }
        }
    }
    Ok(Some(order_id))
}

pub fn get_batch_orders(
    conn: &ConnType,
    ids: &[String],
    platform: &str,
) -> DbResult<Vec<DbBatchOrderItem>> {
    let batch_orders: Vec<DbBatchOrderItem> = oidsl::pay_batch_order_item
        .filter(oidsl::payment_id.eq_any(ids))
        .load(conn)?;

    Ok(batch_orders)
}

impl<'c> BatchDao<'c> {
    pub async fn get_batch_order(
        &self,
        batch_order_id: String,
        node_id: NodeId,
    ) -> DbResult<Option<DbBatchOrder>> {
        readonly_transaction(self.pool, "batch_dao_get", move |conn| {
            Ok(dsl::pay_batch_order
                .filter(dsl::owner_id.eq(node_id).and(dsl::id.eq(batch_order_id)))
                .first(conn)
                .optional()?)
        })
        .await
    }

    pub async fn get_batch_order_items(
        &self,
        batch_order_id: String,
        node_id: NodeId,
    ) -> DbResult<Vec<DbBatchOrderItem>> {
        readonly_transaction(self.pool, "batch_dao_get_items", move |conn| {
            Ok(oidsl::pay_batch_order_item
                .filter(
                    oidsl::owner_id
                        .eq(node_id)
                        .and(oidsl::order_id.eq(batch_order_id)),
                )
                .load(conn)?)
        })
        .await
    }

    pub async fn get_batch_order_items_by_payment_id(
        &self,
        payment_id: String,
        node_id: NodeId,
    ) -> DbResult<Vec<DbBatchOrderItem>> {
        readonly_transaction(self.pool, "batch_dao_get_items", move |conn| {
            Ok(oidsl::pay_batch_order_item
                .filter(
                    oidsl::owner_id
                        .eq(node_id)
                        .and(oidsl::payment_id.eq(payment_id)),
                )
                .load(conn)?)
        })
        .await
    }

    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_items: Option<u32>,
    ) -> DbResult<Vec<DbBatchOrder>> {
        readonly_transaction(self.pool, "batch_dao_get_for_node_id", move |conn| {
            let mut query = dsl::pay_batch_order
                .filter(dsl::owner_id.eq(node_id))
                .into_boxed();
            if let Some(date) = after_timestamp {
                query = query.filter(dsl::ts.gt(date))
            }
            if let Some(items) = max_items {
                query = query.limit(items.into())
            }
            query = query.order_by(dsl::ts.desc());
            Ok(query.load(conn)?)
        })
        .await
    }

    pub async fn resolve(
        &self,
        owner_id: NodeId,
        payer_addr: String,
        platform: String,
        since: DateTime<Utc>,
    ) -> DbResult<Option<String>> {
        do_with_transaction(self.pool, "batch_dao_resolve", move |conn| {
            resolve_invoices(&ResolveInvoiceArgs {
                conn,
                owner_id,
                payer_addr: &payer_addr,
                platform: &platform,
                since,
            })
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

        do_with_transaction(self.pool, "last_debit_notes", move |conn| {
            let v: Vec<Activity> = diesel::sql_query(r#"
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

    pub async fn get_batch_order_payments(
        &self,
        order_id: String,
        owner_id: NodeId,
        payee_addr: String,
    ) -> DbResult<BatchPayment> {
        readonly_transaction(self.pool, "get_batch_order_payments", move |conn| {
            use crate::schema::pay_agreement::dsl as pa;
            use crate::schema::pay_batch_order_item::dsl as di;
            use crate::schema::pay_batch_order_item_document::dsl as d;

            let (amount,) = di::pay_batch_order_item
                .filter(
                    di::order_id
                        .eq(&order_id)
                        .and(di::payee_addr.eq(&payee_addr))
                        .and(di::owner_id.eq(&owner_id)),
                )
                .select((di::amount,))
                .get_result::<(BigDecimalField,)>(conn)?;

            let mut peer_obligation = HashMap::<NodeId, Vec<BatchPaymentObligation>>::new();

            for (payee_id, agreement_id, invoice_id, activity_id, debit_note_id, amount) in
                d::pay_batch_order_item_document
                    .filter(d::order_id.eq(order_id).and(d::payee_addr.eq(payee_addr)))
                    .inner_join(
                        pa::pay_agreement
                            .on(d::owner_id.eq(pa::owner_id).and(d::agreement_id.eq(pa::id))),
                    )
                    .select((
                        pa::peer_id,
                        d::agreement_id,
                        d::invoice_id,
                        d::activity_id,
                        d::debit_note_id,
                        d::amount,
                    ))
                    .load::<(
                        NodeId,
                        String,
                        Option<String>,
                        Option<String>,
                        Option<String>,
                        BigDecimalField,
                    )>(conn)?
            {
                let obligation = if let Some(activity_id) = activity_id {
                    BatchPaymentObligation::DebitNote {
                        debit_note_id,
                        amount: amount.0,
                        agreement_id,
                        activity_id,
                    }
                } else if let Some(invoice_id) = invoice_id {
                    BatchPaymentObligation::Invoice {
                        amount: amount.0,
                        agreement_id,
                        id: invoice_id,
                    }
                } else {
                    return Err(DbError::Integrity("No invoice or activity id".to_string()));
                };
                peer_obligation
                    .entry(payee_id)
                    .or_default()
                    .push(obligation);
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
        readonly_transaction(self.pool, "get_unsent_batch_items", move |conn| {
            use crate::schema::pay_batch_order::dsl as odsl;

            let order: DbBatchOrder = odsl::pay_batch_order
                .filter(odsl::id.eq(&order_id))
                .get_result(conn)?;
            let items: Vec<DbBatchOrderItem> = oidsl::pay_batch_order_item
                .filter(oidsl::order_id.eq(&order_id))
                .filter(oidsl::payment_id.is_null())
                .filter(oidsl::paid.eq(false))
                .load(conn)?;
            Ok((order, items))
        })
        .await
    }

    pub async fn get_batch_items(
        &self,
        owner_id: NodeId,
        order_id: Option<String>,
        payee_addr: Option<String>,
        agreement_id: Option<String>,
        activity_id: Option<String>,
    ) -> DbResult<Vec<DbAgreementBatchOrderItem>> {
        readonly_transaction(self.pool, "get_batch_items_filtered", move |conn| {
            use crate::schema::pay_batch_order::dsl as order_dsl;
            use crate::schema::pay_batch_order_item::dsl as order_item_dsl;
            use crate::schema::pay_batch_order_item_document::dsl as aggr_item_dsl;
            let mut query = order_item_dsl::pay_batch_order_item
                .filter(order_item_dsl::owner_id.eq(owner_id))
                .inner_join(
                    aggr_item_dsl::pay_batch_order_item_document.on(order_item_dsl::order_id
                        .eq(aggr_item_dsl::order_id)
                        .and(order_item_dsl::owner_id.eq(aggr_item_dsl::owner_id))
                        .and(order_item_dsl::payee_addr.eq(aggr_item_dsl::payee_addr))),
                )
                .inner_join(
                    order_dsl::pay_batch_order.on(order_item_dsl::order_id
                        .eq(order_dsl::id)
                        .and(order_item_dsl::owner_id.eq(order_dsl::owner_id))),
                )
                .into_boxed();

            if let Some(order_id) = order_id {
                query = query.filter(order_item_dsl::order_id.eq(order_id));
            }
            if let Some(payee_addr) = payee_addr {
                query = query.filter(order_item_dsl::payee_addr.eq(payee_addr));
            }
            if let Some(agreement_id) = agreement_id {
                query = query.filter(aggr_item_dsl::agreement_id.eq(agreement_id));
            }
            if let Some(activity_id) = activity_id {
                query = query.filter(aggr_item_dsl::activity_id.eq(activity_id));
            }

            Ok(query
                .select((
                    order_dsl::ts,
                    order_item_dsl::order_id,
                    order_item_dsl::owner_id,
                    order_item_dsl::payee_addr,
                    aggr_item_dsl::amount,
                    aggr_item_dsl::agreement_id,
                    aggr_item_dsl::invoice_id,
                    aggr_item_dsl::activity_id,
                    aggr_item_dsl::debit_note_id,
                ))
                .order_by(order_dsl::ts.desc())
                .load(conn)?)
        })
        .await
    }

    pub async fn batch_order_item_send(
        &self,
        order_id: String,
        owner_id: NodeId,
        payee_addr: String,
        payment_id: String,
    ) -> DbResult<usize> {
        do_with_transaction(self.pool, "batch_order_item_send", move |conn| {
            Ok(diesel::update(oidsl::pay_batch_order_item)
                .filter(
                    oidsl::order_id
                        .eq(order_id)
                        .and(oidsl::payee_addr.eq(payee_addr))
                        .and(oidsl::owner_id.eq(owner_id)),
                )
                .set(oidsl::payment_id.eq(payment_id))
                .execute(conn)?)
        })
        .await
    }

    pub async fn batch_order_item_paid(
        &self,
        order_id: String,
        owner_id: NodeId,
        payee_addr: String,
    ) -> DbResult<bool> {
        do_with_transaction(self.pool, "batch_order_item_paid", move |conn| {
            use crate::schema::pay_batch_order::dsl as odsl;
            //use crate::schema::pay_batch_order_item_document::dsl as d;
            let order: DbBatchOrder = odsl::pay_batch_order
                .filter(odsl::id.eq(&order_id))
                .get_result(conn)?;

            let updated_count = diesel::update(oidsl::pay_batch_order_item)
                .filter(
                    oidsl::order_id
                        .eq(&order_id)
                        .and(oidsl::payee_addr.eq(&payee_addr))
                        .and(oidsl::paid.eq(false)),
                )
                .set(oidsl::paid.eq(true))
                .execute(conn)?;
            if updated_count == 0 {
                return Ok(false);
            }
            if updated_count > 2 {
                return Err(DbError::Integrity("More than 1 rows updated".to_string()));
            }

            /*
            let query = d::pay_batch_order_item_document
                .filter(
                    d::order_id.eq(&order_id).and(
                        d::payee_addr
                            .eq(&payee_addr)
                            .and(d::owner_id.eq(&order.owner_id)),
                    ),
                )
                .select((
                    d::payee_addr,
                    d::agreement_id,
                    d::invoice_id,
                    d::activity_id,
                    d::debit_note_id,
                    d::amount,
                ))
                .load::<(
                    NodeId,
                    String,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    BigDecimalField,
                )>(conn)?;
            for (payee_id, agreement_id, invoice_id, activity_id, _debit_note_id, amount) in query {
                if let Some(activity_id) = activity_id {
                    log::warn!(
                        "Increasing amount paid for activity {} {}",
                        activity_id,
                        amount
                    );
                    super::activity::increase_amount_paid(
                        &activity_id,
                        &order.owner_id,
                        &amount,
                        conn,
                    )?;
                }
                super::agreement::increase_amount_paid(
                    &agreement_id,
                    &order.owner_id,
                    &amount,
                    conn,
                )?;
            }*/

            Ok(true)
        })
        .await
    }
}
