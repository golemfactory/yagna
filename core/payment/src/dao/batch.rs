use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Text, Timestamp};
use std::collections::{hash_map, HashMap};
use std::iter::zip;
use std::str::FromStr;
use uuid::Uuid;
use ya_core_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{AdaptTimestamp, BigDecimalField};

use crate::error::{DbError, DbResult};
use crate::models::allocation::AllocationExpenditureObj;
use crate::models::batch::*;
use crate::schema::pay_allocation::dsl as padsl;
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

#[derive(Debug, Clone, Default)]
pub struct BatchItemFilter {
    pub order_id: Option<String>,
    pub payee_addr: Option<String>,
    pub allocation_id: Option<String>,
    pub agreement_id: Option<String>,
    pub activity_id: Option<String>,
    pub payment_id: Option<String>,
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
    if invoices.len() > 0 {
        log::info!("found [{}] invoices", invoices.len());
    }

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
            continue;
        }

        total_amount += &amount_to_pay;

        let obligation = BatchPaymentObligation::Invoice {
            id: invoice_id,
            amount: amount_to_pay.clone(),
            agreement_id: agreement_id.clone(),
        };

        let payee_addr_n = NodeId::from_str(&payee_addr).map_err(|e| {
            log::error!("Error parsing payee_addr: {}", e);
            DbError::Integrity("payee address parsing error".to_string())
        })?;

        match payments.entry(payee_addr) {
            hash_map::Entry::Occupied(mut e) => {
                let payment = e.get_mut();
                payment.amount += &amount_to_pay;
                match payment.peer_obligation.entry(payee_addr_n) {
                    hash_map::Entry::Occupied(mut e) => e.get_mut().push(obligation),
                    hash_map::Entry::Vacant(e) => {
                        e.insert(vec![obligation]);
                    }
                }
            }
            hash_map::Entry::Vacant(e) => {
                let mut peer_obligation = HashMap::new();
                peer_obligation.insert(payee_addr_n, vec![obligation]);
                let amount = amount_to_pay.clone();
                e.insert(BatchPayment {
                    amount,
                    peer_obligation,
                });
            }
        }
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

        if query_res.len() > 0 {
            log::info!("Pay for activities - {} found to check", query_res.len());
        }
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
            if amount_to_pay == zero {
                // Nothing left to schedule for this activity. Same rule as the invoice part,
                // where amounts equal to zero are skipped as well.
                continue;
            }
            total_amount += &amount_to_pay;
            let obligation = BatchPaymentObligation::DebitNote {
                debit_note_id: a.debit_note_id,
                amount: amount_to_pay.clone(),
                agreement_id: a.agreement_id.clone(),
                activity_id: a.id,
            };

            let payee_addr = NodeId::from_str(&a.payee_addr).map_err(|e| {
                log::error!("Error parsing payee_addr: {}", e);
                DbError::Integrity("payee address parsing error".to_string())
            })?;

            match payments.entry(a.payee_addr) {
                hash_map::Entry::Occupied(mut e) => {
                    let payment = e.get_mut();
                    payment.amount += &amount_to_pay;
                    match payment.peer_obligation.entry(payee_addr) {
                        hash_map::Entry::Occupied(mut e) => e.get_mut().push(obligation),
                        hash_map::Entry::Vacant(e) => {
                            e.insert(vec![obligation]);
                        }
                    }
                }
                hash_map::Entry::Vacant(e) => {
                    let mut peer_obligation = HashMap::new();
                    peer_obligation.insert(payee_addr, vec![obligation]);
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct AllocationPayeeKey {
    pub payee_addr: NodeId,
    pub allocation_id: String,
}

fn insert_or_update_allocation_entry(
    payment_allocations: &mut HashMap<AllocationPayeeKey, BatchPaymentAllocation>,
    allocation_payer_key: AllocationPayeeKey,
    obligation: BatchPaymentObligationAllocation,
) -> DbResult<()> {
    let payer_all = payment_allocations
        .entry(allocation_payer_key.clone())
        .or_default();
    let all = payer_all
        .peer_obligation
        .entry(allocation_payer_key.payee_addr)
        .or_default();
    match obligation {
        BatchPaymentObligationAllocation::Invoice {
            id,
            amount,
            agreement_id,
            allocation_id,
        } => {
            payer_all.amount += amount.clone();
            all.push(BatchPaymentObligationAllocation::Invoice {
                id,
                amount,
                agreement_id,
                allocation_id,
            });
        }
        BatchPaymentObligationAllocation::DebitNote {
            debit_note_id,
            amount,
            agreement_id,
            activity_id,
            allocation_id,
        } => {
            payer_all.amount += amount.clone();
            all.push(BatchPaymentObligationAllocation::DebitNote {
                debit_note_id,
                amount,
                agreement_id,
                activity_id,
                allocation_id,
            });
        }
    }

    Ok(())
}

fn use_expenditures_on_payments(
    expenditures: &mut [AllocationExpenditureObj],
    payments: HashMap<String, BatchPayment>,
) -> DbResult<HashMap<AllocationPayeeKey, BatchPaymentAllocation>> {
    let mut payments_allocations: HashMap<AllocationPayeeKey, BatchPaymentAllocation> =
        HashMap::new();
    let mut payments = payments;
    for payment in &mut payments {
        let batch_payment = payment.1;
        for peer_obligation in &mut batch_payment.peer_obligation {
            for obligation in peer_obligation.1 {
                let matching_expenditures = match &obligation {
                    BatchPaymentObligation::Invoice {
                        id,
                        amount,
                        agreement_id,
                    } => expenditures
                        .iter_mut()
                        .filter(|e| {
                            e.agreement_id == agreement_id.clone()
                                && e.activity_id.is_none()
                                && e.accepted_amount.0 > e.scheduled_amount.0
                        })
                        .collect::<Vec<&mut AllocationExpenditureObj>>(),
                    BatchPaymentObligation::DebitNote {
                        debit_note_id,
                        amount,
                        agreement_id,
                        activity_id,
                    } => expenditures
                        .iter_mut()
                        .filter(|e| {
                            e.agreement_id == agreement_id.clone()
                                && e.activity_id == Some(activity_id.clone())
                                && e.accepted_amount.0 > e.scheduled_amount.0
                        })
                        .collect::<Vec<&mut AllocationExpenditureObj>>(),
                };
                log::info!(
                    "Found {} matching expenditures for obligation {:?}",
                    matching_expenditures.len(),
                    obligation
                );
                let amount_to_be_covered = match &obligation {
                    BatchPaymentObligation::Invoice {
                        id,
                        amount,
                        agreement_id,
                    } => amount,
                    BatchPaymentObligation::DebitNote {
                        debit_note_id,
                        amount,
                        agreement_id,
                        activity_id,
                    } => amount,
                };
                let mut amount_covered = BigDecimal::from(0u32);
                for expenditure in matching_expenditures {
                    if amount_covered >= *amount_to_be_covered {
                        break;
                    }
                    let max_amount_to_get = expenditure.accepted_amount.0.clone()
                        - expenditure.scheduled_amount.0.clone();

                    let cover_amount = std::cmp::min(
                        amount_to_be_covered.clone() - amount_covered.clone(),
                        max_amount_to_get,
                    );
                    expenditure.scheduled_amount =
                        (expenditure.scheduled_amount.0.clone() + cover_amount.clone()).into();

                    match &obligation {
                        BatchPaymentObligation::Invoice {
                            id,
                            amount,
                            agreement_id,
                        } => {
                            insert_or_update_allocation_entry(
                                &mut payments_allocations,
                                AllocationPayeeKey {
                                    payee_addr: *peer_obligation.0,
                                    allocation_id: expenditure.allocation_id.clone(),
                                },
                                BatchPaymentObligationAllocation::Invoice {
                                    id: id.clone(),
                                    amount: cover_amount.clone(),
                                    agreement_id: agreement_id.clone(),
                                    allocation_id: expenditure.allocation_id.clone(),
                                },
                            )?;
                        }
                        BatchPaymentObligation::DebitNote {
                            debit_note_id,
                            amount,
                            agreement_id,
                            activity_id,
                        } => {
                            insert_or_update_allocation_entry(
                                &mut payments_allocations,
                                AllocationPayeeKey {
                                    payee_addr: *peer_obligation.0,
                                    allocation_id: expenditure.allocation_id.clone(),
                                },
                                BatchPaymentObligationAllocation::DebitNote {
                                    debit_note_id: debit_note_id.clone(),
                                    amount: cover_amount.clone(),
                                    agreement_id: agreement_id.clone(),
                                    activity_id: activity_id.clone(),
                                    allocation_id: expenditure.allocation_id.clone(),
                                },
                            )?;
                        }
                    }
                    match &obligation {
                        BatchPaymentObligation::Invoice {
                            id,
                            amount,
                            agreement_id,
                        } => {
                            log::info!("Covered invoice obligation {} with {} of {} from allocation {} - agreement id: {}", id, cover_amount, amount, expenditure.allocation_id, agreement_id);
                        }
                        BatchPaymentObligation::DebitNote {
                            debit_note_id,
                            amount,
                            agreement_id,
                            activity_id,
                        } => {
                            log::info!("Covered debit note obligation {:?} with {} of {} from allocation {} - agreement id: {} - activity id: {}", debit_note_id, cover_amount, amount, expenditure.allocation_id, agreement_id, activity_id);
                        }
                    }
                    amount_covered += cover_amount;
                }
                match &obligation {
                    BatchPaymentObligation::Invoice {
                        id,
                        amount,
                        agreement_id,
                    } => {
                        log::info!(
                            "Total covered invoice obligation {} with {} of {} from allocations",
                            id,
                            amount_covered,
                            amount
                        );
                    }
                    BatchPaymentObligation::DebitNote {
                        debit_note_id,
                        amount,
                        agreement_id,
                        activity_id,
                    } => {
                        log::info!("Total covered debit note obligation {:?} with {} of {} from allocations", debit_note_id, amount_covered, amount);
                    }
                }
            }
        }
    }
    Ok(payments_allocations)
}

fn schedule_covered_payments(
    conn: &ConnType,
    owner_id: &NodeId,
    payments: &HashMap<AllocationPayeeKey, BatchPaymentAllocation>,
) -> DbResult<BigDecimal> {
    let mut total_amount = BigDecimal::from(0u32);

    for payment in payments.values() {
        total_amount += &payment.amount;
        for obligations in payment.peer_obligation.values() {
            for obligation in obligations {
                match obligation {
                    BatchPaymentObligationAllocation::Invoice {
                        amount,
                        agreement_id,
                        ..
                    } => {
                        super::agreement::increase_amount_scheduled(
                            agreement_id,
                            owner_id,
                            amount,
                            conn,
                        )?;
                    }
                    BatchPaymentObligationAllocation::DebitNote {
                        amount,
                        activity_id,
                        ..
                    } => {
                        super::activity::increase_amount_scheduled(
                            activity_id,
                            owner_id,
                            amount,
                            conn,
                        )?;
                    }
                }
            }
        }
    }

    Ok(total_amount)
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

    log::debug!("Resolving invoices for {} - {}", owner_id, platform);
    let (payments, total_amount) = resolve_invoices_activity_part(args, total_amount, payments)?;

    log::debug!("Resolving agreements for {}", owner_id);
    let (payments, total_amount) = resolve_invoices_agreement_part(args, total_amount, payments)?;

    if total_amount == zero {
        return Ok(None);
    }

    // Get allocation expenditures. The resolver window applies to the obligation queries above,
    // not to allocations: a still-valid allocation may legitimately be older than `since`.

    use crate::schema::pay_allocation::dsl as pa_dsl;
    use crate::schema::pay_allocation_expenditure::dsl as pae_dsl;
    let expenditures_orig: Vec<AllocationExpenditureObj> = pae_dsl::pay_allocation_expenditure
        .select(pae_dsl::pay_allocation_expenditure::all_columns())
        .inner_join(
            crate::schema::pay_allocation::dsl::pay_allocation.on(pae_dsl::allocation_id
                .eq(pa_dsl::id)
                .and(pae_dsl::owner_id.eq(pa_dsl::owner_id))
                .and(pa_dsl::payment_platform.eq(args.platform))
                .and(pa_dsl::owner_id.eq(args.owner_id))),
        )
        .filter(pae_dsl::accepted_amount.ne(pae_dsl::scheduled_amount))
        .load(conn)?;
    let mut expenditures = expenditures_orig.clone();

    if expenditures.len() > 0 {
        log::debug!("Found total of {} expenditures", expenditures.len());
    }

    let payments_allocations =
        use_expenditures_on_payments(&mut expenditures, payments).map_err(|e| {
            log::error!("Error using expenditures on payments: {:?}", e);
            e
        })?;

    let total_amount = schedule_covered_payments(conn, &owner_id, &payments_allocations)?;
    if total_amount == zero {
        return Ok(None);
    }

    // upload the updated expenditures to database (if changed)
    for (expenditure_new, expenditure_old) in zip(expenditures.iter(), expenditures_orig.iter()) {
        if expenditure_new.scheduled_amount != expenditure_old.scheduled_amount {
            log::debug!("Updating expenditure {:?}", expenditure_new);
            let mut query = diesel::update(pae_dsl::pay_allocation_expenditure)
                .filter(pae_dsl::owner_id.eq(&expenditure_new.owner_id))
                .filter(pae_dsl::allocation_id.eq(&expenditure_new.allocation_id))
                .filter(pae_dsl::agreement_id.eq(&expenditure_new.agreement_id))
                .into_boxed();
            if let Some(activity_id) = &expenditure_new.activity_id {
                query = query.filter(pae_dsl::activity_id.eq(activity_id));
            } else {
                query = query.filter(pae_dsl::activity_id.is_null());
            }
            query
                .set(pae_dsl::scheduled_amount.eq(&expenditure_new.scheduled_amount))
                .execute(conn)?;
        } else {
            log::debug!("Expenditure {:?} not changed", expenditure_new);
        }
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
        for (key, payment) in payments_allocations {
            let payee_addr = key.payee_addr;
            let allocation_id = key.allocation_id;
            diesel::insert_into(oidsl::pay_batch_order_item)
                .values((
                    oidsl::order_id.eq(&order_id),
                    oidsl::owner_id.eq(owner_id),
                    oidsl::payee_addr.eq(&payee_addr),
                    oidsl::amount.eq(BigDecimalField(payment.amount.clone())),
                    oidsl::allocation_id.eq(&allocation_id),
                ))
                .execute(conn)?;
            for (payee_id, obligations) in payment.peer_obligation {
                for obligation in &obligations {
                    log::debug!("obligation: {:?}", obligation);
                    match obligation {
                        BatchPaymentObligationAllocation::Invoice {
                            id,
                            amount,
                            agreement_id,
                            allocation_id,
                        } => {
                            use crate::schema::pay_batch_order_item_document::dsl;
                            diesel::insert_into(dsl::pay_batch_order_item_document)
                                .values((
                                    dsl::order_id.eq(&order_id),
                                    dsl::owner_id.eq(owner_id),
                                    dsl::payee_addr.eq(&payee_addr),
                                    dsl::allocation_id.eq(allocation_id),
                                    dsl::agreement_id.eq(agreement_id),
                                    dsl::invoice_id.eq(id),
                                    dsl::activity_id.eq(None::<String>),
                                    dsl::debit_note_id.eq(None::<String>),
                                    dsl::amount.eq(BigDecimalField(amount.clone())),
                                ))
                                .execute(conn)?;
                        }
                        BatchPaymentObligationAllocation::DebitNote {
                            amount,
                            debit_note_id,
                            agreement_id,
                            activity_id,
                            allocation_id,
                        } => {
                            use crate::schema::pay_batch_order_item_document::dsl;
                            diesel::insert_into(dsl::pay_batch_order_item_document)
                                .values((
                                    dsl::order_id.eq(&order_id),
                                    dsl::owner_id.eq(owner_id),
                                    dsl::payee_addr.eq(&payee_addr),
                                    dsl::allocation_id.eq(allocation_id),
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

impl BatchDao<'_> {
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
                query = query.filter(dsl::created_ts.gt(date))
            }
            if let Some(items) = max_items {
                query = query.limit(items.into())
            }
            query = query.order_by(dsl::created_ts.desc());
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

    pub async fn get_unsent_batch_items(
        &self,
        owner_id: NodeId,
        order_id: String,
    ) -> DbResult<Vec<DbBatchOrderItemFullInfo>> {
        readonly_transaction(self.pool, "get_unsent_batch_items", move |conn| {
            Ok(oidsl::pay_batch_order_item
                .inner_join(
                    padsl::pay_allocation.on(oidsl::allocation_id
                        .eq(padsl::id)
                        .and(oidsl::owner_id.eq(padsl::owner_id))),
                )
                .inner_join(
                    dsl::pay_batch_order.on(oidsl::order_id
                        .eq(dsl::id)
                        .and(oidsl::owner_id.eq(dsl::owner_id))
                        .and(dsl::owner_id.eq(owner_id))
                        .and(dsl::id.eq(&order_id))),
                )
                .select((
                    oidsl::order_id,
                    dsl::platform,
                    oidsl::owner_id,
                    dsl::payer_addr,
                    oidsl::payee_addr,
                    oidsl::allocation_id,
                    padsl::deposit,
                    oidsl::amount,
                    oidsl::payment_id,
                    oidsl::paid,
                ))
                .filter(
                    oidsl::owner_id
                        .eq(owner_id)
                        .and(oidsl::order_id.eq(&order_id))
                        .and(oidsl::payment_id.is_null())
                        .and(oidsl::paid.eq(false)),
                )
                .load::<DbBatchOrderItemFullInfo>(conn)?)
        })
        .await
    }

    pub async fn get_batch_items(
        &self,
        owner_id: NodeId,
        filter: BatchItemFilter,
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
                        .and(order_item_dsl::allocation_id.eq(aggr_item_dsl::allocation_id))
                        .and(order_item_dsl::payee_addr.eq(aggr_item_dsl::payee_addr))),
                )
                .inner_join(
                    order_dsl::pay_batch_order.on(order_item_dsl::order_id
                        .eq(order_dsl::id)
                        .and(order_item_dsl::owner_id.eq(order_dsl::owner_id))),
                )
                .into_boxed();

            if let Some(order_id) = filter.order_id {
                query = query.filter(order_item_dsl::order_id.eq(order_id));
            }
            if let Some(payee_addr) = filter.payee_addr {
                query = query.filter(order_item_dsl::payee_addr.eq(payee_addr));
            }
            if let Some(allocation_id) = filter.allocation_id {
                query = query.filter(order_item_dsl::allocation_id.eq(allocation_id));
            }
            if let Some(agreement_id) = filter.agreement_id {
                query = query.filter(aggr_item_dsl::agreement_id.eq(agreement_id));
            }
            if let Some(activity_id) = filter.activity_id {
                query = query.filter(aggr_item_dsl::activity_id.eq(activity_id));
            }
            if let Some(payment_id) = filter.payment_id {
                query = query.filter(order_item_dsl::payment_id.eq(payment_id));
            }

            Ok(query
                .select((
                    order_dsl::created_ts,
                    order_dsl::updated_ts,
                    order_item_dsl::order_id,
                    order_item_dsl::owner_id,
                    order_item_dsl::payee_addr,
                    order_item_dsl::allocation_id,
                    aggr_item_dsl::amount,
                    aggr_item_dsl::agreement_id,
                    aggr_item_dsl::invoice_id,
                    aggr_item_dsl::activity_id,
                    aggr_item_dsl::debit_note_id,
                ))
                .order_by(order_dsl::created_ts.desc())
                .load(conn)?)
        })
        .await
    }

    pub async fn batch_order_item_send(
        &self,
        order_id: String,
        owner_id: NodeId,
        payee_addr: String,
        allocation_id: String,
        payment_id: String,
    ) -> DbResult<usize> {
        do_with_transaction(self.pool, "batch_order_item_send", move |conn| {
            Ok(diesel::update(oidsl::pay_batch_order_item)
                .filter(
                    oidsl::order_id
                        .eq(order_id)
                        .and(oidsl::payee_addr.eq(payee_addr))
                        .and(oidsl::allocation_id.eq(allocation_id))
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
        allocation_id: String,
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
                        .and(oidsl::allocation_id.eq(allocation_id.clone()))
                        .and(oidsl::owner_id.eq(owner_id))
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

            let current_order_item = oidsl::pay_batch_order_item
                .filter(
                    oidsl::order_id
                        .eq(&order_id)
                        .and(oidsl::payee_addr.eq(&payee_addr))
                        .and(oidsl::allocation_id.eq(allocation_id))
                        .and(oidsl::owner_id.eq(owner_id)),
                )
                .first::<DbBatchOrderItem>(conn)?;

            //update amount paid on batch order
            let current_order = dsl::pay_batch_order
                .filter(dsl::id.eq(&order_id).and(dsl::owner_id.eq(owner_id)))
                .get_result::<DbBatchOrder>(conn)?;

            let updated_amount = current_order.paid_amount + current_order_item.amount;
            let now = Utc::now().adapt();
            diesel::update(dsl::pay_batch_order)
                .filter(dsl::id.eq(&order_id).and(dsl::owner_id.eq(owner_id)))
                .set((dsl::paid_amount.eq(updated_amount), dsl::updated_ts.eq(now)))
                .execute(conn)?;

            Ok(true)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use ya_persistence::executor::DbExecutor;

    const AGREEMENT_ID: &str = "agreement-1";
    const ALLOCATION_ID: &str = "allocation-1";
    const INVOICE_ID: &str = "invoice-1";
    const ACTIVITY_ID: &str = "activity-1";
    const PLATFORM: &str = "erc20-test-tglm";
    const REQUESTOR: &str = "0x1000000000000000000000000000000000000000";
    const PROVIDER: &str = "0x2000000000000000000000000000000000000000";

    fn requestor_id() -> NodeId {
        REQUESTOR.parse().unwrap()
    }

    async fn test_db(name: &str) -> DbExecutor {
        let db = DbExecutor::in_memory(&format!("{name}-{}", Uuid::new_v4())).unwrap();
        db.apply_migration(crate::migrations::run_with_output)
            .unwrap();
        db
    }

    async fn insert_agreement(db: &DbExecutor, accepted: &'static str, updated_ts: NaiveDateTime) {
        do_with_transaction(&db.pool, "insert_test_agreement", move |conn| {
            use crate::schema::pay_agreement::dsl as agreement;

            diesel::insert_into(agreement::pay_agreement)
                .values((
                    agreement::id.eq(AGREEMENT_ID),
                    agreement::owner_id.eq(requestor_id()),
                    agreement::role.eq("R"),
                    agreement::peer_id.eq(PROVIDER),
                    agreement::payee_addr.eq(PROVIDER),
                    agreement::payer_addr.eq(REQUESTOR),
                    agreement::payment_platform.eq(PLATFORM),
                    agreement::total_amount_due.eq(accepted),
                    agreement::total_amount_accepted.eq(accepted),
                    agreement::total_amount_scheduled.eq("0"),
                    agreement::total_amount_paid.eq("0"),
                    agreement::app_session_id.eq(None::<String>),
                    agreement::created_ts.eq(Some(updated_ts)),
                    agreement::updated_ts.eq(Some(updated_ts)),
                ))
                .execute(conn)?;
            Ok::<_, DbError>(())
        })
        .await
        .unwrap();
    }

    async fn insert_allocation_and_expenditure(
        db: &DbExecutor,
        updated_ts: NaiveDateTime,
        activity_id: Option<&'static str>,
        accepted: &'static str,
    ) {
        do_with_transaction(&db.pool, "insert_test_allocation", move |conn| {
            use crate::schema::pay_allocation::dsl as allocation;
            use crate::schema::pay_allocation_expenditure::dsl as expenditure;

            diesel::insert_into(allocation::pay_allocation)
                .values((
                    allocation::id.eq(ALLOCATION_ID),
                    allocation::owner_id.eq(requestor_id()),
                    allocation::payment_platform.eq(PLATFORM),
                    allocation::address.eq(REQUESTOR),
                    allocation::avail_amount.eq("0"),
                    allocation::spent_amount.eq(accepted),
                    allocation::created_ts.eq(updated_ts),
                    allocation::updated_ts.eq(updated_ts),
                    allocation::timeout.eq(updated_ts + Duration::days(365)),
                    allocation::released.eq(false),
                    allocation::deposit.eq(None::<String>),
                    allocation::deposit_status.eq(None::<String>),
                ))
                .execute(conn)?;

            diesel::insert_into(expenditure::pay_allocation_expenditure)
                .values((
                    expenditure::owner_id.eq(requestor_id()),
                    expenditure::allocation_id.eq(ALLOCATION_ID),
                    expenditure::agreement_id.eq(AGREEMENT_ID),
                    expenditure::activity_id.eq(activity_id),
                    expenditure::accepted_amount.eq(accepted),
                    expenditure::scheduled_amount.eq("0"),
                ))
                .execute(conn)?;
            Ok::<_, DbError>(())
        })
        .await
        .unwrap();
    }

    async fn insert_invoice(db: &DbExecutor, timestamp: NaiveDateTime) {
        do_with_transaction(&db.pool, "insert_test_invoice", move |conn| {
            use crate::schema::pay_invoice::dsl as invoice;

            diesel::insert_into(invoice::pay_invoice)
                .values((
                    invoice::id.eq(INVOICE_ID),
                    invoice::owner_id.eq(requestor_id()),
                    invoice::role.eq("R"),
                    invoice::agreement_id.eq(AGREEMENT_ID),
                    invoice::status.eq("ACCEPTED"),
                    invoice::timestamp.eq(timestamp),
                    invoice::amount.eq("10"),
                    invoice::payment_due_date.eq(timestamp + Duration::days(1)),
                    invoice::send_accept.eq(false),
                    invoice::send_reject.eq(false),
                ))
                .execute(conn)?;
            Ok::<_, DbError>(())
        })
        .await
        .unwrap();
    }

    fn received_invoice(
        amount: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> ya_client_model::payment::Invoice {
        ya_client_model::payment::Invoice {
            invoice_id: INVOICE_ID.to_string(),
            issuer_id: PROVIDER.parse().unwrap(),
            recipient_id: requestor_id(),
            payee_addr: PROVIDER.to_string(),
            payer_addr: REQUESTOR.to_string(),
            payment_platform: PLATFORM.to_string(),
            timestamp,
            agreement_id: AGREEMENT_ID.to_string(),
            activity_ids: vec![],
            amount,
            payment_due_date: timestamp + Duration::days(1),
            status: ya_client_model::payment::DocumentStatus::Received,
        }
    }

    async fn invoice_status(db: &DbExecutor) -> String {
        do_with_transaction(&db.pool, "read_test_invoice_status", move |conn| {
            use crate::schema::pay_invoice::dsl as invoice;

            Ok::<_, DbError>(
                invoice::pay_invoice
                    .find((INVOICE_ID, requestor_id()))
                    .select(invoice::status)
                    .first::<String>(conn)?,
            )
        })
        .await
        .unwrap()
    }

    async fn scheduled_amounts(db: &DbExecutor) -> (BigDecimalField, BigDecimalField) {
        do_with_transaction(&db.pool, "read_test_scheduled_amounts", move |conn| {
            use crate::schema::pay_agreement::dsl as agreement;
            use crate::schema::pay_allocation_expenditure::dsl as expenditure;

            let agreement_amount = agreement::pay_agreement
                .find((AGREEMENT_ID, requestor_id()))
                .select(agreement::total_amount_scheduled)
                .first(conn)?;
            let expenditure_amount = expenditure::pay_allocation_expenditure
                .filter(expenditure::owner_id.eq(requestor_id()))
                .filter(expenditure::allocation_id.eq(ALLOCATION_ID))
                .filter(expenditure::agreement_id.eq(AGREEMENT_ID))
                .select(expenditure::scheduled_amount)
                .first(conn)?;
            Ok::<_, DbError>((agreement_amount, expenditure_amount))
        })
        .await
        .unwrap()
    }

    #[actix_rt::test]
    async fn fresh_invoice_uses_allocation_older_than_resolver_window() {
        let db = test_db("batch-old-allocation").await;
        let now = Utc::now();
        let since = now - Duration::days(30);
        insert_agreement(&db, "10", now.naive_utc()).await;
        insert_allocation_and_expenditure(&db, (now - Duration::days(31)).naive_utc(), None, "10")
            .await;
        insert_invoice(&db, now.naive_utc()).await;

        let dao = db.as_dao::<BatchDao>();
        let order_id = dao
            .resolve(requestor_id(), REQUESTOR.into(), PLATFORM.into(), since)
            .await
            .unwrap()
            .expect("fresh invoice should produce a batch order");
        let order = dao
            .get_batch_order(order_id.clone(), requestor_id())
            .await
            .unwrap()
            .unwrap();
        let items = dao
            .get_batch_order_items(order_id, requestor_id())
            .await
            .unwrap();

        assert_eq!(order.total_amount.0, BigDecimal::from(10));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].allocation_id, ALLOCATION_ID);
        assert_eq!(items[0].amount.0, BigDecimal::from(10));
        let (agreement_scheduled, expenditure_scheduled) = scheduled_amounts(&db).await;
        assert_eq!(agreement_scheduled.0, BigDecimal::from(10));
        assert_eq!(expenditure_scheduled.0, BigDecimal::from(10));
    }

    #[actix_rt::test]
    async fn invoice_older_than_resolver_window_is_not_scheduled() {
        let db = test_db("batch-old-invoice").await;
        let now = Utc::now();
        let since = now - Duration::days(30);
        insert_agreement(&db, "10", now.naive_utc()).await;
        insert_allocation_and_expenditure(&db, now.naive_utc(), None, "10").await;
        insert_invoice(&db, (now - Duration::days(31)).naive_utc()).await;

        let order_id = db
            .as_dao::<BatchDao>()
            .resolve(requestor_id(), REQUESTOR.into(), PLATFORM.into(), since)
            .await
            .unwrap();

        assert!(order_id.is_none());
        let (agreement_scheduled, expenditure_scheduled) = scheduled_amounts(&db).await;
        assert_eq!(agreement_scheduled.0, BigDecimal::from(0));
        assert_eq!(expenditure_scheduled.0, BigDecimal::from(0));
    }

    #[actix_rt::test]
    async fn activity_with_nothing_left_to_schedule_is_skipped() {
        let db = test_db("batch-zero-delta-activity").await;
        let now = Utc::now();
        let since = now - Duration::days(30);
        insert_agreement(&db, "10", now.naive_utc()).await;

        // `0.0` and `0` are numerically equal but differ as text, so this row passes the
        // string comparisons in the activity query and yields a zero amount to pay.
        do_with_transaction(&db.pool, "insert_test_activity", move |conn| {
            use crate::schema::pay_activity::dsl as activity;

            diesel::insert_into(activity::pay_activity)
                .values((
                    activity::id.eq(ACTIVITY_ID),
                    activity::owner_id.eq(requestor_id()),
                    activity::role.eq("R"),
                    activity::agreement_id.eq(AGREEMENT_ID),
                    activity::total_amount_due.eq("0.0"),
                    activity::total_amount_accepted.eq("0.0"),
                    activity::total_amount_scheduled.eq("0"),
                    activity::total_amount_paid.eq("0"),
                    activity::created_ts.eq(Some(now.naive_utc())),
                    activity::updated_ts.eq(Some(now.naive_utc())),
                ))
                .execute(conn)?;
            Ok::<_, DbError>(())
        })
        .await
        .unwrap();
        insert_allocation_and_expenditure(&db, now.naive_utc(), Some(ACTIVITY_ID), "10").await;

        let order_id = db
            .as_dao::<BatchDao>()
            .resolve(requestor_id(), REQUESTOR.into(), PLATFORM.into(), since)
            .await
            .unwrap();

        assert!(order_id.is_none());
        let (agreement_scheduled, expenditure_scheduled) = scheduled_amounts(&db).await;
        assert_eq!(agreement_scheduled.0, BigDecimal::from(0));
        assert_eq!(expenditure_scheduled.0, BigDecimal::from(0));
    }

    #[actix_rt::test]
    async fn activity_on_agreement_older_than_resolver_window_is_not_scheduled() {
        let db = test_db("batch-old-activity").await;
        let now = Utc::now();
        let since = now - Duration::days(30);
        insert_agreement(&db, "10", (now - Duration::days(31)).naive_utc()).await;

        do_with_transaction(&db.pool, "insert_test_activity", move |conn| {
            use crate::schema::pay_activity::dsl as activity;

            diesel::insert_into(activity::pay_activity)
                .values((
                    activity::id.eq(ACTIVITY_ID),
                    activity::owner_id.eq(requestor_id()),
                    activity::role.eq("R"),
                    activity::agreement_id.eq(AGREEMENT_ID),
                    activity::total_amount_due.eq("10"),
                    activity::total_amount_accepted.eq("10"),
                    activity::total_amount_scheduled.eq("0"),
                    activity::total_amount_paid.eq("0"),
                    activity::created_ts.eq(Some(now.naive_utc())),
                    activity::updated_ts.eq(Some(now.naive_utc())),
                ))
                .execute(conn)?;
            Ok::<_, DbError>(())
        })
        .await
        .unwrap();
        insert_allocation_and_expenditure(&db, now.naive_utc(), Some(ACTIVITY_ID), "10").await;

        let order_id = db
            .as_dao::<BatchDao>()
            .resolve(requestor_id(), REQUESTOR.into(), PLATFORM.into(), since)
            .await
            .unwrap();

        assert!(order_id.is_none());
        let (agreement_scheduled, expenditure_scheduled) = scheduled_amounts(&db).await;
        assert_eq!(agreement_scheduled.0, BigDecimal::from(0));
        assert_eq!(expenditure_scheduled.0, BigDecimal::from(0));
    }

    #[actix_rt::test]
    async fn only_the_amount_covered_by_an_expenditure_is_scheduled() {
        let db = test_db("batch-partial-coverage").await;
        let now = Utc::now();
        let since = now - Duration::days(30);
        insert_agreement(&db, "10", now.naive_utc()).await;
        insert_allocation_and_expenditure(&db, now.naive_utc(), None, "4").await;
        insert_invoice(&db, now.naive_utc()).await;

        let dao = db.as_dao::<BatchDao>();
        let order_id = dao
            .resolve(requestor_id(), REQUESTOR.into(), PLATFORM.into(), since)
            .await
            .unwrap()
            .expect("covered invoice amount should produce a batch order");
        let order = dao
            .get_batch_order(order_id.clone(), requestor_id())
            .await
            .unwrap()
            .unwrap();
        let items = dao
            .get_batch_order_items(order_id, requestor_id())
            .await
            .unwrap();

        assert_eq!(order.total_amount.0, BigDecimal::from(4));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].amount.0, BigDecimal::from(4));
        let (agreement_scheduled, expenditure_scheduled) = scheduled_amounts(&db).await;
        assert_eq!(agreement_scheduled.0, BigDecimal::from(4));
        assert_eq!(expenditure_scheduled.0, BigDecimal::from(4));
    }

    #[actix_rt::test]
    async fn zero_amount_invoice_is_accepted_and_settled_without_a_batch_order() {
        let db = test_db("batch-zero-invoice").await;
        let now = Utc::now();
        let since = now - Duration::days(30);
        insert_agreement(&db, "0", now.naive_utc()).await;
        insert_allocation_and_expenditure(&db, now.naive_utc(), None, "0").await;

        let invoice_dao = db.as_dao::<crate::dao::InvoiceDao>();
        invoice_dao
            .insert_received(received_invoice(BigDecimal::from(0), now))
            .await
            .expect("zero-amount invoice must be accepted on receipt");
        invoice_dao
            .accept(INVOICE_ID.to_string(), requestor_id())
            .await
            .expect("zero-amount invoice must be acceptable");

        assert_eq!(invoice_status(&db).await, "SETTLED");

        let order_id = db
            .as_dao::<BatchDao>()
            .resolve(requestor_id(), REQUESTOR.into(), PLATFORM.into(), since)
            .await
            .unwrap();
        assert!(order_id.is_none(), "nothing to pay for a zero invoice");
    }

    #[actix_rt::test]
    async fn negative_amount_invoice_is_rejected_on_receipt() {
        let db = test_db("batch-negative-invoice").await;
        let now = Utc::now();
        insert_agreement(&db, "0", now.naive_utc()).await;

        let err = db
            .as_dao::<crate::dao::InvoiceDao>()
            .insert_received(received_invoice(BigDecimal::from(-5), now))
            .await
            .expect_err("negative invoice must be rejected");
        assert!(
            matches!(&err, DbError::Query(msg) if msg.contains("cannot be negative")),
            "expected a bad-request-mapped rejection, got: {}",
            err
        );
    }

    #[actix_rt::test]
    async fn negative_amount_debit_note_is_rejected_on_receipt() {
        let db = test_db("batch-negative-debit-note").await;
        let now = Utc::now();
        insert_agreement(&db, "0", now.naive_utc()).await;

        let debit_note = ya_client_model::payment::DebitNote {
            debit_note_id: "debit-note-1".to_string(),
            issuer_id: PROVIDER.parse().unwrap(),
            recipient_id: requestor_id(),
            payee_addr: PROVIDER.to_string(),
            payer_addr: REQUESTOR.to_string(),
            payment_platform: PLATFORM.to_string(),
            previous_debit_note_id: None,
            timestamp: now,
            agreement_id: AGREEMENT_ID.to_string(),
            activity_id: ACTIVITY_ID.to_string(),
            total_amount_due: BigDecimal::from(-5),
            usage_counter_vector: None,
            payment_due_date: Some(now + Duration::days(1)),
            status: ya_client_model::payment::DocumentStatus::Received,
        };

        let err = db
            .as_dao::<crate::dao::DebitNoteDao>()
            .insert_received(debit_note)
            .await
            .expect_err("negative debit note must be rejected");
        assert!(
            matches!(&err, DbError::Query(msg) if msg.contains("cannot be negative")),
            "expected a bad-request-mapped rejection, got: {}",
            err
        );
    }

    #[actix_rt::test]
    async fn negative_amount_invoice_cannot_be_accepted() {
        let db = test_db("batch-negative-invoice-accept").await;
        let now = Utc::now();
        let since = now - Duration::days(30);
        insert_agreement(&db, "0", now.naive_utc()).await;
        insert_allocation_and_expenditure(&db, now.naive_utc(), None, "0").await;

        // Force a negative invoice into the DB, bypassing the receipt-time guard,
        // to check what the resolver would do with it.
        do_with_transaction(&db.pool, "insert_negative_invoice", move |conn| {
            use crate::schema::pay_invoice::dsl as invoice;

            diesel::insert_into(invoice::pay_invoice)
                .values((
                    invoice::id.eq(INVOICE_ID),
                    invoice::owner_id.eq(requestor_id()),
                    invoice::role.eq("R"),
                    invoice::agreement_id.eq(AGREEMENT_ID),
                    invoice::status.eq("ACCEPTED"),
                    invoice::timestamp.eq(now.naive_utc()),
                    invoice::amount.eq("-5"),
                    invoice::payment_due_date.eq(now.naive_utc() + Duration::days(1)),
                    invoice::send_accept.eq(false),
                    invoice::send_reject.eq(false),
                ))
                .execute(conn)?;
            Ok::<_, DbError>(())
        })
        .await
        .unwrap();

        let order_id = db
            .as_dao::<BatchDao>()
            .resolve(requestor_id(), REQUESTOR.into(), PLATFORM.into(), since)
            .await
            .unwrap();
        assert!(order_id.is_none(), "negative invoice must not be paid");
        let (agreement_scheduled, expenditure_scheduled) = scheduled_amounts(&db).await;
        assert_eq!(agreement_scheduled.0, BigDecimal::from(0));
        assert_eq!(expenditure_scheduled.0, BigDecimal::from(0));
    }
}
