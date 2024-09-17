use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Text, Timestamp};
use std::collections::{hash_map, HashMap};
use std::iter::zip;
use uuid::Uuid;
use ya_core_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::BigDecimalField;

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
        log::info!(
            "Increase amount scheduled agreement_id={} by {}",
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
            log::info!("Increasing amount scheduled for activity: {}, amount {} (acc: {}, sch: {})", a.id, amount_to_pay, a.total_amount_accepted.0, a.total_amount_scheduled.0);
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

    //get allocation expenditures

    use crate::schema::pay_allocation::dsl as pa_dsl;
    use crate::schema::pay_allocation_expenditure::dsl as pae_dsl;
    let expenditures_orig: Vec<AllocationExpenditureObj> = pae_dsl::pay_allocation_expenditure
        .select(pae_dsl::pay_allocation_expenditure::all_columns())
        .inner_join(
            crate::schema::pay_allocation::dsl::pay_allocation.on(pae_dsl::allocation_id
                .eq(pa_dsl::id)
                .and(pae_dsl::owner_id.eq(pa_dsl::owner_id))
                .and(pa_dsl::payment_platform.eq(args.platform))
                .and(pa_dsl::updated_ts.gt(args.since.naive_utc()))
                .and(pa_dsl::owner_id.eq(args.owner_id))),
        )
        .filter(pae_dsl::accepted_amount.ne(pae_dsl::scheduled_amount))
        .load(conn)?;
    let mut expenditures = expenditures_orig.clone();

    log::info!("Found total of {} expenditures", expenditures.len());

    let payments_allocations =
        use_expenditures_on_payments(&mut expenditures, payments).map_err(|e| {
            log::error!("Error using expenditures on payments: {:?}", e);
            e
        })?;

    // upload the updated expenditures to database (if changed)
    for (expenditure_new, expenditure_old) in zip(expenditures.iter(), expenditures_orig.iter()) {
        if expenditure_new.scheduled_amount != expenditure_old.scheduled_amount {
            log::info!("Updating expenditure {:?}", expenditure_new);
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
            log::info!("Expenditure {:?} not changed", expenditure_new);
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

            Ok(query
                .select((
                    order_dsl::ts,
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
                        .and(oidsl::allocation_id.eq(allocation_id))
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
