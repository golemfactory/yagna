use crate::error::DbResult;
use crate::models::*;
use crate::schema::pay_debit_note::dsl;
use diesel::{self, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
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

    pub async fn insert(&self, debit_note: DebitNote) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            // TODO: Check previous_debit_note_id
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

    pub async fn get_status(&self) -> DbResult<()> {
        do_with_transaction(self.pool, |conn| todo!()).await
    }
}
