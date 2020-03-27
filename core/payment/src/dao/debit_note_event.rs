use crate::error::DbResult;
use crate::models::*;
use crate::schema::pay_debit_note::dsl as debit_note_dsl;
use crate::schema::pay_debit_note_event::dsl;
use chrono::NaiveDateTime;
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl};
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

pub struct DebitNoteEventDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for DebitNoteEventDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> DebitNoteEventDao<'c> {
    pub async fn create(&self, event: NewDebitNoteEvent) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::pay_debit_note_event)
                .values(event)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn get_for_recipient(
        &self,
        recipient_id: String,
        later_than: Option<NaiveDateTime>,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        do_with_transaction(self.pool, move |conn| {
            let query = dsl::pay_debit_note_event
                .inner_join(debit_note_dsl::pay_debit_note)
                .filter(debit_note_dsl::recipient_id.eq(recipient_id))
                .select(crate::schema::pay_debit_note_event::all_columns);
            let events = match later_than {
                Some(timestamp) => query.filter(dsl::timestamp.gt(timestamp)).load(conn)?,
                None => query.load(conn)?,
            };
            Ok(events)
        })
        .await
    }

    pub async fn get_for_issuer(
        &self,
        issuer_id: String,
        later_than: Option<NaiveDateTime>,
    ) -> DbResult<Vec<DebitNoteEvent>> {
        do_with_transaction(self.pool, move |conn| {
            let query = dsl::pay_debit_note_event
                .inner_join(debit_note_dsl::pay_debit_note)
                .filter(debit_note_dsl::issuer_id.eq(issuer_id))
                .select(crate::schema::pay_debit_note_event::all_columns);
            let events = match later_than {
                Some(timestamp) => query.filter(dsl::timestamp.gt(timestamp)).load(conn)?,
                None => query.load(conn)?,
            };
            Ok(events)
        })
        .await
    }
}
