use crate::dao::{activity, agreement};
use crate::error::DbResult;
use crate::models::payment::{DocumentPayment, ReadObj, WriteObj};
use crate::schema::pay_activity::dsl as activity_dsl;
use crate::schema::pay_agreement::dsl as agreement_dsl;
use crate::schema::pay_payment::dsl;
use crate::schema::pay_payment_document::dsl as document_pay_dsl;

use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use diesel::{
    BoolExpressionMethods, ExpressionMethods, JoinOnDsl, OptionalExtension, QueryDsl, RunQueryDsl,
    TextExpressionMethods,
};
use std::collections::HashMap;
use ya_client_model::payment::{ActivityPayment, AgreementPayment, Payment, Signed};
use ya_client_model::NodeId;
use ya_core_model::payment::local::{DriverName, NetworkName};
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};
use ya_persistence::types::{BigDecimalField, Role};

pub struct PaymentDao<'c> {
    pool: &'c PoolType,
}

fn insert_activity_payments(
    activity_payments: Vec<ActivityPayment>,
    payment_id: &str,
    owner_id: &NodeId,
    conn: &ConnType,
) -> DbResult<()> {
    log::trace!("Inserting activity payments...");
    for activity_payment in activity_payments.iter() {
        let amount: BigDecimalField = activity_payment.amount.clone().into();
        let allocation_id = activity_payment.allocation_id.clone();

        let agreement_id: String = activity_dsl::pay_activity
            .select(activity_dsl::agreement_id)
            .filter(activity_dsl::id.eq(&activity_payment.activity_id))
            .filter(activity_dsl::owner_id.eq(owner_id))
            .first(conn)
            .map_err(|e| {
                log::error!(
                    "Error getting agreement_id for activity_id: {}, owner_id: {}. Error: {}",
                    activity_payment.activity_id,
                    owner_id,
                    e
                );
                e
            })?;

        agreement::increase_amount_paid(&agreement_id, owner_id, &amount, conn)?;
        activity::increase_amount_paid(&activity_payment.activity_id, owner_id, &amount, conn)?;

        diesel::insert_into(document_pay_dsl::pay_payment_document)
            .values(DocumentPayment {
                payment_id: payment_id.to_string(),
                agreement_id: agreement_id.clone(),
                invoice_id: None,
                activity_id: Some(activity_payment.activity_id.clone()),
                owner_id: *owner_id,
                amount,
                debit_note_id: None,
            })
            .execute(conn)
            .map(|_| ())?;
    }
    log::trace!("Activity payments inserted.");
    Ok(())
}

fn insert_agreement_payments(
    agreement_payments: Vec<AgreementPayment>,
    payment_id: &str,
    owner_id: &NodeId,
    conn: &ConnType,
) -> DbResult<()> {
    log::trace!("Inserting agreement payments...");
    for agreement_payment in agreement_payments {
        let amount = agreement_payment.amount.into();
        let allocation_id = agreement_payment.allocation_id;

        agreement::increase_amount_paid(&agreement_payment.agreement_id, owner_id, &amount, conn)?;

        diesel::insert_into(document_pay_dsl::pay_payment_document)
            .values(DocumentPayment {
                payment_id: payment_id.to_string(),
                agreement_id: agreement_payment.agreement_id,
                invoice_id: None,
                activity_id: None,
                owner_id: *owner_id,
                amount,
                debit_note_id: None,
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

        do_with_transaction(self.pool, "payment_dao_insert", move |conn| {
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
            None,
            None,
        );
        let payment_id = payment.id.clone();
        self.insert(payment, activity_payments, agreement_payments)
            .await?;
        Ok(payment_id)
    }

    pub async fn insert_received(
        &self,
        payment: Payment,
        payee_id: NodeId,
        signature: Option<Vec<u8>>,
        signed_bytes: Option<Vec<u8>>,
    ) -> DbResult<()> {
        let activity_payments = payment.activity_payments.clone();
        let agreement_payments = payment.agreement_payments.clone();
        let payment = WriteObj::new_received(payment, signature, signed_bytes)?;
        self.insert(payment, activity_payments, agreement_payments)
            .await
    }

    pub async fn mark_sent(&self, payment_id: String) -> DbResult<()> {
        do_with_transaction(self.pool, "payment_dao_mark_sent", move |conn| {
            diesel::update(dsl::pay_payment.filter(dsl::id.eq(payment_id)))
                .filter(dsl::role.eq(Role::Requestor))
                .set(dsl::send_payment.eq(false))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn add_signature(
        &self,
        payment_id: String,
        signature: Vec<u8>,
        signed_bytes: Vec<u8>,
    ) -> DbResult<()> {
        do_with_transaction(self.pool, "payment_dao_update", move |conn| {
            diesel::update(dsl::pay_payment.filter(dsl::id.eq(payment_id)))
                .filter(dsl::role.eq(Role::Requestor))
                .set((
                    dsl::signature.eq(signature),
                    dsl::signed_bytes.eq(signed_bytes),
                ))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get(
        &self,
        payment_id: String,
        owner_id: NodeId,
    ) -> DbResult<Option<Signed<Payment>>> {
        readonly_transaction(self.pool, "payment_dao_get", move |conn| {
            let payment: Option<ReadObj> = dsl::pay_payment
                .filter(dsl::id.eq(&payment_id))
                .filter(dsl::owner_id.eq(&owner_id))
                .first(conn)
                .optional()?;

            match payment {
                Some(payment) => {
                    let document_payments = document_pay_dsl::pay_payment_document
                        .filter(document_pay_dsl::payment_id.eq(&payment_id))
                        .filter(document_pay_dsl::owner_id.eq(&owner_id))
                        .load(conn)?;
                    Ok(Some(payment.into_signed_api_model(document_payments)))
                }
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn get_for_confirmation(
        &self,
        details: Vec<u8>,
        role: Role,
    ) -> DbResult<Vec<Payment>> {
        readonly_transaction(self.pool, "payment_dao_get_for_confirmation", move |conn| {
            let mut result = Vec::new();

            let payments: Vec<ReadObj> = dsl::pay_payment
                .filter(dsl::details.eq(&details))
                .filter(dsl::role.eq(&role.to_string()))
                .load(conn)?;

            for payment in payments {
                let document_payments = document_pay_dsl::pay_payment_document
                    .filter(document_pay_dsl::payment_id.eq(&payment.id))
                    .filter(document_pay_dsl::owner_id.eq(&payment.owner_id))
                    .load(conn)?;

                result.push(payment.into_api_model(document_payments))
            }

            Ok(result)
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
    ) -> DbResult<Vec<Signed<Payment>>> {
        readonly_transaction(self.pool, "payment_dao_get_for_node_id", move |conn| {
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

            let document_payments = {
                let mut query = document_pay_dsl::pay_payment_document
                    .inner_join(
                        dsl::pay_payment.on(document_pay_dsl::owner_id
                            .eq(dsl::owner_id)
                            .and(document_pay_dsl::payment_id.eq(dsl::id))),
                    )
                    .inner_join(
                        agreement_dsl::pay_agreement.on(document_pay_dsl::owner_id
                            .eq(agreement_dsl::owner_id)
                            .and(document_pay_dsl::agreement_id.eq(agreement_dsl::id))),
                    )
                    .filter(dsl::owner_id.eq(&node_id))
                    .select(crate::schema::pay_payment_document::all_columns)
                    .into_boxed();
                if let Some(app_session_id) = &app_session_id {
                    query = query.filter(agreement_dsl::app_session_id.eq(app_session_id));
                }
                query.load(conn)?
            };

            let mut payments = join_document_payments(payments, document_payments);

            // A trick to filter payments by app_session_id. Payments are not directly linked to any
            // particular agreement but they always have at least one related activity_payment or
            // agreement_payment. So if some payment lacks related sub-payments that means they've
            // been filtered out due to non-matching app_sesion_id and so should be the payment.
            if let Some(app_session_id) = app_session_id {
                payments.retain(|s| {
                    let payment = &s.payload;
                    !payment.activity_payments.is_empty() || !payment.agreement_payments.is_empty()
                });
            };

            Ok(payments)
        })
        .await
    }

    pub async fn list_unsent(
        &self,
        owner: NodeId,
        peer_id: Option<NodeId>,
    ) -> DbResult<Vec<Payment>> {
        readonly_transaction(self.pool, "payment_dao_list_unsent", move |conn| {
            let mut query = dsl::pay_payment
                .filter(dsl::send_payment.eq(true))
                .filter(dsl::owner_id.eq(&owner))
                .into_boxed();
            if let Some(peer_id) = peer_id {
                query = query.filter(dsl::peer_id.eq(&peer_id));
            }

            let read: Vec<ReadObj> = query.load(conn)?;

            let mut payments = Vec::default();
            for payment in read {
                let document_payments = document_pay_dsl::pay_payment_document
                    .filter(document_pay_dsl::payment_id.eq(&payment.id))
                    .filter(document_pay_dsl::owner_id.eq(&payment.owner_id))
                    .load(conn)?;

                payments.push(payment.into_api_model(document_payments))
            }

            Ok(payments)
        })
        .await
    }
}

#[allow(clippy::unwrap_or_default)]
fn join_document_payments(
    payments: Vec<ReadObj>,
    document_payments: Vec<DocumentPayment>,
) -> Vec<Signed<Payment>> {
    let mut document_payments_map =
        document_payments
            .into_iter()
            .fold(HashMap::new(), |mut map, document_payment| {
                map.entry(document_payment.payment_id.clone())
                    .or_insert_with(Vec::new)
                    .push(document_payment);
                map
            });

    payments
        .into_iter()
        .map(|payment| {
            let document_payments = document_payments_map
                .remove(&payment.id)
                .unwrap_or_default();
            payment.into_signed_api_model(document_payments)
        })
        .collect()
}
