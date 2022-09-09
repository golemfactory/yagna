use crate::dao::{activity, agreement};
use crate::error::DbResult;
use crate::models::payment::{
    ActivityPayment as DbActivityPayment, AgreementPayment as DbAgreementPayment, ReadObj, WriteObj,
};
use crate::schema::pay_activity::dsl as activity_dsl;
use crate::schema::pay_activity_payment::dsl as activity_pay_dsl;
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_agreement_payment::dsl as agreement_pay_dsl;
use crate::schema::pay_payment::dsl;
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::{
    BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl, RunQueryDsl,
    TextExpressionMethods,
};
use std::collections::HashMap;
use ya_client_model::payment::{ActivityPayment, AgreementPayment, Payment};
use ya_client_model::NodeId;
use ya_core_model::payment::local::{DriverName, NetworkName};
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

pub struct PaymentDao<'c> {
    pool: &'c PoolType,
}

fn insert_activity_payments(
    activity_payments: Vec<ActivityPayment>,
    payment_id: &String,
    owner_id: &NodeId,
    conn: &ConnType,
) -> DbResult<()> {
    log::trace!("Inserting activity payments...");
    for activity_payment in activity_payments {
        let amount = activity_payment.amount.into();
        let allocation_id = activity_payment.allocation_id;

        activity::increase_amount_paid(&activity_payment.activity_id, owner_id, &amount, conn)?;

        diesel::insert_into(activity_pay_dsl::pay_activity_payment)
            .values(DbActivityPayment {
                payment_id: payment_id.clone(),
                activity_id: activity_payment.activity_id,
                owner_id: *owner_id,
                amount,
                allocation_id,
            })
            .execute(conn)
            .map(|_| ())?;
    }
    log::trace!("Activity payments inserted.");
    Ok(())
}

fn insert_agreement_payments(
    agreement_payments: Vec<AgreementPayment>,
    payment_id: &String,
    owner_id: &NodeId,
    conn: &ConnType,
) -> DbResult<()> {
    log::trace!("Inserting agreement payments...");
    for agreement_payment in agreement_payments {
        let amount = agreement_payment.amount.into();
        let allocation_id = agreement_payment.allocation_id;

        agreement::increase_amount_paid(&agreement_payment.agreement_id, owner_id, &amount, conn)?;

        diesel::insert_into(agreement_pay_dsl::pay_agreement_payment)
            .values(DbAgreementPayment {
                payment_id: payment_id.clone(),
                agreement_id: agreement_payment.agreement_id,
                owner_id: *owner_id,
                amount,
                allocation_id,
            })
            .execute(conn)
            .map(|_| ())?;
    }
    log::trace!("Agreement payments inserted.");
    Ok(())
}

impl<'c> AsDao<'c> for PaymentDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> PaymentDao<'c> {
    async fn insert(
        &self,
        payment: WriteObj,
        activity_payments: Vec<ActivityPayment>,
        agreement_payments: Vec<AgreementPayment>,
    ) -> DbResult<()> {
        let payment_id = payment.id.clone();
        let owner_id = payment.owner_id;
        let amount = payment.amount.clone();

        do_with_transaction(self.pool, move |conn| {
            log::trace!("Inserting payment...");
            diesel::insert_into(dsl::pay_payment)
                .values(payment)
                .execute(conn)?;
            log::trace!("Payment inserted.");

            insert_activity_payments(activity_payments, &payment_id, &owner_id, conn)?;
            insert_agreement_payments(agreement_payments, &payment_id, &owner_id, conn)?;

            Ok(())
        })
        .await
    }

    //TODO Rafał
    #[allow(clippy::too_many_arguments)]
    pub async fn create_new(
        &self,
        payer_id: NodeId,
        payee_id: NodeId,
        payer_addr: String,
        payee_addr: String,
        payment_platform: String,
        amount: BigDecimal,
        details: Vec<u8>,
        activity_payments: Vec<ActivityPayment>,
        agreement_payments: Vec<AgreementPayment>,
    ) -> DbResult<String> {
        let payment = WriteObj::new_sent(
            payer_id,
            payee_id,
            payer_addr,
            payee_addr,
            payment_platform,
            amount,
            details,
        );
        let payment_id = payment.id.clone();
        self.insert(payment, activity_payments, agreement_payments)
            .await?;
        Ok(payment_id)
    }

    pub async fn insert_received(&self, payment: Payment, payee_id: NodeId) -> DbResult<()> {
        let activity_payments = payment.activity_payments.clone();
        let agreement_payments = payment.agreement_payments.clone();
        let payment = WriteObj::new_received(payment)?;
        self.insert(payment, activity_payments, agreement_payments)
            .await
    }

    pub async fn get(&self, payment_id: String, owner_id: NodeId) -> DbResult<Option<Payment>> {
        readonly_transaction(self.pool, move |conn| {
            let payment: Option<ReadObj> = dsl::pay_payment
                .filter(dsl::id.eq(&payment_id))
                .filter(dsl::owner_id.eq(&owner_id))
                .first(conn)
                .optional()?;

            match payment {
                Some(payment) => {
                    let activity_payments = activity_pay_dsl::pay_activity_payment
                        .filter(activity_pay_dsl::payment_id.eq(&payment_id))
                        .filter(activity_pay_dsl::owner_id.eq(&owner_id))
                        .load(conn)?;
                    let agreement_payments = agreement_pay_dsl::pay_agreement_payment
                        .filter(agreement_pay_dsl::payment_id.eq(&payment_id))
                        .filter(agreement_pay_dsl::owner_id.eq(&owner_id))
                        .load(conn)?;
                    Ok(Some(
                        payment.into_api_model(activity_payments, agreement_payments),
                    ))
                }
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn get_for_node_id(
        &self,
        node_id: NodeId,
        after_timestamp: Option<NaiveDateTime>,
        max_events: Option<u32>,
        app_session_id: Option<String>,
        network: Option<NetworkName>,
        driver: Option<DriverName>,
    ) -> DbResult<Vec<Payment>> {
        readonly_transaction(self.pool, move |conn| {
            let mut query = dsl::pay_payment
                .filter(dsl::owner_id.eq(&node_id))
                .order_by(dsl::timestamp.asc())
                .into_boxed();
            if let Some(timestamp) = after_timestamp {
                query = query.filter(dsl::timestamp.gt(timestamp));
            }
            if let Some(limit) = max_events {
                query = query.limit(limit.into());
            }
            if let Some(network) = network {
                query = query.filter(dsl::payment_platform.like(format!("{}%", network)));
            }
            if let Some(driver) = driver {
                query = query.filter(dsl::payment_platform.like(format!("%{}%", driver)));
            }

            let payments: Vec<ReadObj> = query.load(conn)?;

            let activity_payments = {
                let mut query = activity_pay_dsl::pay_activity_payment
                    .inner_join(
                        dsl::pay_payment.on(activity_pay_dsl::owner_id
                            .eq(dsl::owner_id)
                            .and(activity_pay_dsl::payment_id.eq(dsl::id))),
                    )
                    .inner_join(
                        activity_dsl::pay_activity.on(activity_pay_dsl::owner_id
                            .eq(activity_dsl::owner_id)
                            .and(activity_pay_dsl::activity_id.eq(activity_dsl::id))),
                    )
                    .inner_join(
                        agreement_dsl::pay_agreement.on(activity_pay_dsl::owner_id
                            .eq(agreement_dsl::owner_id)
                            .and(activity_dsl::agreement_id.eq(agreement_dsl::id))),
                    )
                    .filter(dsl::owner_id.eq(&node_id))
                    .select(crate::schema::pay_activity_payment::all_columns)
                    .into_boxed();
                if let Some(app_session_id) = &app_session_id {
                    query = query.filter(agreement_dsl::app_session_id.eq(app_session_id));
                }
                query.load(conn)?
            };

            let agreement_payments = {
                let mut query = agreement_pay_dsl::pay_agreement_payment
                    .inner_join(
                        dsl::pay_payment.on(agreement_pay_dsl::owner_id
                            .eq(dsl::owner_id)
                            .and(agreement_pay_dsl::payment_id.eq(dsl::id))),
                    )
                    .inner_join(
                        agreement_dsl::pay_agreement.on(agreement_pay_dsl::owner_id
                            .eq(agreement_dsl::owner_id)
                            .and(agreement_pay_dsl::agreement_id.eq(agreement_dsl::id))),
                    )
                    .filter(dsl::owner_id.eq(&node_id))
                    .select(crate::schema::pay_agreement_payment::all_columns)
                    .into_boxed();
                if let Some(app_session_id) = &app_session_id {
                    query = query.filter(agreement_dsl::app_session_id.eq(app_session_id));
                }
                query.load(conn)?
            };

            let mut payments = join_activity_and_agreement_payments(
                payments,
                activity_payments,
                agreement_payments,
            );

            // A trick to filter payments by app_session_id. Payments are not directly linked to any
            // particular agreement but they always have at least one related activity_payment or
            // agreement_payment. So if some payment lacks related sub-payments that means they've
            // been filtered out due to non-matching app_sesion_id and so should be the payment.
            if let Some(app_session_id) = app_session_id {
                payments.retain(|payment| {
                    !payment.activity_payments.is_empty() || !payment.agreement_payments.is_empty()
                });
            };

            Ok(payments)
        })
        .await
    }
}

fn join_activity_and_agreement_payments(
    payments: Vec<ReadObj>,
    activity_payments: Vec<DbActivityPayment>,
    agreement_payments: Vec<DbAgreementPayment>,
) -> Vec<Payment> {
    let mut activity_payments_map =
        activity_payments
            .into_iter()
            .fold(HashMap::new(), |mut map, activity_payment| {
                map.entry(activity_payment.payment_id.clone())
                    .or_insert_with(Vec::new)
                    .push(activity_payment);
                map
            });
    let mut agreement_payments_map =
        agreement_payments
            .into_iter()
            .fold(HashMap::new(), |mut map, agreement_payment| {
                map.entry(agreement_payment.payment_id.clone())
                    .or_insert_with(Vec::new)
                    .push(agreement_payment);
                map
            });
    payments
        .into_iter()
        .map(|payment| {
            let activity_payments = activity_payments_map
                .remove(&payment.id)
                .unwrap_or_default();
            let agreement_payments = agreement_payments_map
                .remove(&payment.id)
                .unwrap_or_default();
            payment.into_api_model(activity_payments, agreement_payments)
        })
        .collect()
}
