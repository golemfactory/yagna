use crate::error::DbResult;
use crate::models::*;
use crate::schema::pay_debit_note::dsl;
use bigdecimal::BigDecimal;
use diesel::sql_types::Text;
use diesel::{self, sql_query, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use std::collections::HashMap;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::payment::local::StatusNotes;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

pub struct DebitNoteDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for DebitNoteDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> DebitNoteDao<'c> {
    pub async fn create(&self, mut debit_note: NewDebitNote) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            // TODO: Move previous_debit_note_id assignment to database trigger
            debit_note.previous_debit_note_id = dsl::pay_debit_note
                .select(dsl::id)
                .filter(dsl::agreement_id.eq(debit_note.agreement_id.clone()))
                .order_by(dsl::timestamp.desc())
                .first(conn)
                .optional()?;
            diesel::insert_into(dsl::pay_debit_note)
                .values(debit_note)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn insert(&self, mut debit_note: DebitNote) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            // TODO: Move previous_debit_note_id assignment to database trigger
            debit_note.previous_debit_note_id = dsl::pay_debit_note
                .select(dsl::id)
                .filter(dsl::agreement_id.eq(&debit_note.agreement_id))
                .order_by(dsl::timestamp.desc())
                .first(conn)
                .optional()?;
            diesel::insert_into(dsl::pay_debit_note)
                .values(debit_note)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get(&self, debit_note_id: String) -> DbResult<Option<DebitNote>> {
        do_with_transaction(self.pool, move |conn| {
            let debit_note: Option<DebitNote> = dsl::pay_debit_note
                .find(debit_note_id)
                .first(conn)
                .optional()?;
            Ok(debit_note)
        })
        .await
    }

    pub async fn get_all(&self) -> DbResult<Vec<DebitNote>> {
        do_with_transaction(self.pool, move |conn| {
            let debit_notes: Vec<DebitNote> = dsl::pay_debit_note.load(conn)?;
            Ok(debit_notes)
        })
        .await
    }

    pub async fn get_issued(&self, issuer_id: String) -> DbResult<Vec<DebitNote>> {
        do_with_transaction(self.pool, move |conn| {
            let debit_notes: Vec<DebitNote> = dsl::pay_debit_note
                .filter(dsl::issuer_id.eq(issuer_id))
                .load(conn)?;
            Ok(debit_notes)
        })
        .await
    }

    pub async fn get_received(&self, recipient_id: String) -> DbResult<Vec<DebitNote>> {
        do_with_transaction(self.pool, move |conn| {
            let debit_notes: Vec<DebitNote> = dsl::pay_debit_note
                .filter(dsl::recipient_id.eq(recipient_id))
                .load(conn)?;
            Ok(debit_notes)
        })
        .await
    }

    pub async fn update_status(&self, debit_note_id: String, status: String) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(dsl::pay_debit_note.find(debit_note_id))
                .set(dsl::status.eq(status))
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn status_report(&self, idenity: NodeId) -> DbResult<(StatusNotes, StatusNotes)> {
        log::info!("enter {}", "status_report");
        let r = do_with_transaction(self.pool, move |conn| {
            let notes: Vec<DebitNote> = sql_query(
                r#"
            SELECT *
            FROM pay_debit_note as n
            WHERE status = 'SETTLED'
            AND (issuer_id = ? or recipient_id=?)
            AND NOT EXISTS (SELECT 1 FROM pay_debit_note
            where previous_debit_note_id = n.id and status = 'SETTLED')
            AND NOT EXISTS (SELECT 1
                FROM pay_invoice
                WHERE recipient_id = n.recipient_id
                AND agreement_id = n.agreement_id)
            "#,
            )
            .bind::<Text, _>(&idenity)
            .bind::<Text, _>(&idenity)
            .load(conn)
            .map_err(|e| {
                log::error!("{}: select SETTLED from pay_debit_note", e);
                e
            })?;

            let mut incoming_settled: HashMap<String, BigDecimal> = Default::default();
            let mut outgoing_settled: HashMap<String, BigDecimal> = Default::default();
            let me = idenity.to_string();

            // Phase 1: Collect settled amount
            for note in notes {
                if note.issuer_id == me {
                    incoming_settled.insert(note.agreement_id, note.total_amount_due.0);
                } else if note.recipient_id == me {
                    outgoing_settled.insert(note.agreement_id, note.total_amount_due.0);
                } else {
                    unreachable!()
                }
            }

            let notes: Vec<DebitNote> = sql_query(
                r#"
            SELECT *
            FROM pay_debit_note as n
            WHERE status in ('RECEIVED', 'ACCEPTED','REJECTED')
            AND (issuer_id = ? or recipient_id=?)
            AND NOT EXISTS (
                SELECT 1 FROM pay_debit_note
                WHERE previous_debit_note_id = n.id)
            AND NOT EXISTS (
                SELECT 1 FROM pay_invoice
                WHERE recipient_id = n.recipient_id
                AND agreement_id = n.agreement_id)
            "#,
            )
            .bind::<Text, _>(&idenity)
            .bind::<Text, _>(&idenity)
            .get_results(conn)?;

            let mut incoming = StatusNotes::default();
            let mut outgoing = StatusNotes::default();
            for note in notes {
                let s = if note.issuer_id == me {
                    &mut incoming
                } else {
                    &mut outgoing
                };
                let settled = if note.issuer_id == me {
                    incoming_settled
                        .get(&note.agreement_id)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    outgoing_settled
                        .get(&note.agreement_id)
                        .cloned()
                        .unwrap_or_default()
                };

                let pending_amount = note.total_amount_due.0 - settled;
                match note.status.as_str() {
                    "RECEIVED" => s.requested += pending_amount,
                    "ACCEPTED" => s.accepted += pending_amount,
                    "REJECTED" => s.rejected += pending_amount,
                    _ => (),
                }
            }

            Ok((incoming, outgoing))
        })
        .await;
        log::info!("exit {}", "status_report");
        r
    }
}
