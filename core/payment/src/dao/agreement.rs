use crate::error::DbResult;
use crate::models::agreement::{ReadObj, WriteObj};
use crate::schema::pay_agreement::dsl;
use bigdecimal::{BigDecimal, Zero};
use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use ya_client_model::market::Agreement;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::payment::local::StatusNotes;
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
    assert!(amount > &BigDecimal::zero().into());
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
    assert!(total_amount_due >= &agreement.total_amount_due);
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
    assert!(amount > &BigDecimal::zero().into());
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let total_amount_accepted = &agreement.total_amount_accepted + amount;
    diesel::update(&agreement)
        .set(dsl::total_amount_accepted.eq(total_amount_accepted))
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
    assert!(total_amount_accepted >= &agreement.total_amount_accepted);
    diesel::update(&agreement)
        .set(dsl::total_amount_accepted.eq(total_amount_accepted))
        .execute(conn)?;
    Ok(())
}

pub fn increase_amount_paid(
    agreement_id: &String,
    owner_id: &NodeId,
    amount: &BigDecimalField,
    conn: &ConnType,
) -> DbResult<()> {
    assert!(amount > &BigDecimal::zero().into());
    let agreement: ReadObj = dsl::pay_agreement
        .find((agreement_id, owner_id))
        .first(conn)?;
    let total_amount_paid = &agreement.total_amount_paid + amount;
    diesel::update(&agreement)
        .set(dsl::total_amount_paid.eq(total_amount_paid))
        .execute(conn)?;
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

    /// Get total requested/accepted/paid amount of incoming and outgoing transactions
    pub async fn status_report(&self, node_id: NodeId) -> DbResult<(StatusNotes, StatusNotes)> {
        readonly_transaction(self.pool, move |conn| {
            let agreements: Vec<ReadObj> = dsl::pay_agreement
                .filter(dsl::owner_id.eq(node_id))
                .get_results(conn)?;
            let (incoming, outgoing) = agreements.into_iter().fold(
                (StatusNotes::default(), StatusNotes::default()),
                |(incoming, outgoing), agreement| {
                    let status_notes = StatusNotes {
                        requested: agreement.total_amount_due.into(),
                        accepted: agreement.total_amount_accepted.into(),
                        confirmed: agreement.total_amount_paid.into(),
                        rejected: Default::default(), // TODO: Support rejected amount (?)
                    };
                    match agreement.role {
                        Role::Provider => (incoming + status_notes, outgoing),
                        Role::Requestor => (incoming, outgoing + status_notes),
                    }
                },
            );
            Ok((incoming, outgoing))
        })
        .await
    }
}
