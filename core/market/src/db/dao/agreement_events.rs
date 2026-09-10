use chrono::{NaiveDateTime, Utc};
use diesel::{BoolExpressionMethods, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};

use ya_client::model::market::Reason;
use ya_client::model::NodeId;
use ya_persistence::executor::PoolType;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, ConnType};
use ya_persistence::types::AdaptTimestamp;

use crate::db::dao::AgreementDaoError;
use crate::db::model::{
    Agreement, AgreementEvent, AgreementEventType, AgreementId, DbReason, NewAgreementEvent,
};
use crate::db::model::{AppSessionId, Owner};
use crate::db::schema::market_agreement::dsl as agreement;
use crate::db::schema::market_agreement::dsl::market_agreement;
use crate::db::schema::market_agreement_event::dsl as event;
use crate::db::schema::market_agreement_event::dsl::market_agreement_event;
use crate::db::{AsMixedDao, DbResult};

pub struct AgreementEventsDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsMixedDao<'a> for AgreementEventsDao<'a> {
    fn as_dao(disk_pool: &'a PoolType, _ram_pool: &'a PoolType) -> Self {
        Self { pool: disk_pool }
    }
}

impl AgreementEventsDao<'_> {
    pub async fn select(
        &self,
        node_id: &NodeId,
        session_id: &AppSessionId,
        max_events: i32,
        after_timestamp: NaiveDateTime,
    ) -> DbResult<Vec<AgreementEvent>> {
        let session_id = session_id.clone();
        let node_id = *node_id;
        readonly_transaction(self.pool, "agreement_events_dao_select", move |conn| {
            // We will get only one Agreement, by using this filter.
            // There will be no way to get Requestor'a Agreement, when being Provider, and vice versa,
            // because AgreementId for Provider and Requestor in Agreement Event table is different.
            let filter_my_agreements = agreement::provider_id
                .eq(node_id)
                .or(agreement::requestor_id.eq(node_id));

            let mut select_corresponding_agreement = market_agreement
                .select(agreement::id)
                .filter(filter_my_agreements)
                .into_boxed();

            // Optionally filter by `AppSessionId`.
            if let Some(session_id) = session_id {
                select_corresponding_agreement =
                    select_corresponding_agreement.filter(agreement::session_id.eq(session_id));
            };

            Ok(market_agreement_event
                .filter(event::agreement_id.eq_any(select_corresponding_agreement))
                .filter(event::timestamp.gt(after_timestamp.adapt()))
                .order_by(event::timestamp.asc())
                .limit(max_events as i64)
                .load::<AgreementEvent>(conn)?)
        })
        .await
    }

    pub async fn select_for_agreement(
        &self,
        agreement_id: &AgreementId,
    ) -> DbResult<Vec<AgreementEvent>> {
        let agreement_id = agreement_id.clone();
        readonly_transaction(
            self.pool,
            "agreement_events_dao_select_for_agreement",
            move |conn| {
                Ok(market_agreement_event
                    .filter(event::agreement_id.eq(agreement_id))
                    .order_by(event::timestamp.asc())
                    .load::<AgreementEvent>(conn)?)
            },
        )
        .await
    }

    pub async fn select_termination_notice(
        &self,
        agreement_id: &AgreementId,
    ) -> DbResult<Option<AgreementEvent>> {
        let agreement_id = agreement_id.clone();
        readonly_transaction(
            self.pool,
            "agreement_events_dao_select_termination_notice",
            move |conn| Ok(query_termination_notice(conn, &agreement_id)?),
        )
        .await
    }

    /// Records the termination notice for an Agreement. At most one notice
    /// may exist per Agreement and its payload is immutable: a repeated call
    /// with the same payload is reported as `AlreadyRecorded`, a call with a
    /// different payload as `Conflict`.
    pub async fn create_termination_notice(
        &self,
        agreement_id: &AgreementId,
        termination_deadline: NaiveDateTime,
        reason: Option<Reason>,
    ) -> DbResult<TerminationNoticeOutcome> {
        let agreement_id = agreement_id.clone();
        do_with_transaction(
            self.pool,
            "agreement_events_dao_create_termination_notice",
            move |conn| {
                if let Some(existing) = query_termination_notice(conn, &agreement_id)? {
                    let same_deadline = existing
                        .termination_deadline
                        .map(|deadline| deadline.adapt().format())
                        == Some(termination_deadline.adapt().format());
                    let same_reason = same_reason(&existing.reason, &reason);

                    return Ok(match same_deadline && same_reason {
                        true => TerminationNoticeOutcome::AlreadyRecorded,
                        false => TerminationNoticeOutcome::Conflict,
                    });
                }

                let event = NewAgreementEvent {
                    agreement_id,
                    event_type: AgreementEventType::TerminationNotice,
                    timestamp: Utc::now().adapt(),
                    issuer: Owner::Provider,
                    reason: reason.map(DbReason),
                    termination_deadline: Some(termination_deadline.adapt()),
                };

                diesel::insert_into(market_agreement_event)
                    .values(&event)
                    .execute(conn)?;
                Ok(TerminationNoticeOutcome::Recorded)
            },
        )
        .await
    }
}

/// Result of an attempt to record a termination notice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminationNoticeOutcome {
    /// The notice was recorded now.
    Recorded,
    /// A notice with the same payload was recorded before.
    AlreadyRecorded,
    /// A notice with a different payload is already recorded.
    Conflict,
}

fn query_termination_notice(
    conn: &mut ConnType,
    agreement_id: &AgreementId,
) -> Result<Option<AgreementEvent>, diesel::result::Error> {
    market_agreement_event
        .filter(event::agreement_id.eq(agreement_id))
        .filter(event::event_type.eq(AgreementEventType::TerminationNotice))
        .first::<AgreementEvent>(conn)
        .optional()
}

/// Compares Reasons in their serialized form. Structural equality doesn't
/// survive a database round-trip: `Reason::extra` deserializes an absent
/// `extra` as an empty object even when it started as `Null`.
pub(crate) fn same_reason(recorded: &Option<DbReason>, incoming: &Option<Reason>) -> bool {
    recorded.as_ref().map(|reason| reason.to_string())
        == incoming
            .as_ref()
            .map(|reason| DbReason(reason.clone()).to_string())
}

pub(crate) fn create_event(
    conn: &mut ConnType,
    agreement: &Agreement,
    reason: Option<Reason>,
    terminator: Owner,
    timestamp: NaiveDateTime,
) -> Result<(), AgreementDaoError> {
    let event = NewAgreementEvent::new(agreement, reason, terminator, timestamp)
        .map_err(|e| AgreementDaoError::EventError(e.to_string()))?;

    diesel::insert_into(market_agreement_event)
        .values(&event)
        .execute(conn)
        .map_err(|e| AgreementDaoError::EventError(e.to_string()))?;

    let events = market_agreement_event
        .filter(event::agreement_id.eq(&agreement.id))
        .load::<AgreementEvent>(conn)?;

    for event in events.iter() {
        log::debug!(
            "Event timestamp: {}, type: {}",
            event.timestamp,
            event.event_type
        );
    }

    Ok(())
}
