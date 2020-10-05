use diesel::prelude::*;

use ya_client_model::activity::{RuntimeEvent as RpcRuntimeEvent, RuntimeEventKind};
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::dao::{DaoError, Result};
use crate::db::{
    models::{Activity, RuntimeEventType},
    schema,
};

pub struct RuntimeEventDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for RuntimeEventDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        RuntimeEventDao { pool }
    }
}

impl<'c> RuntimeEventDao<'c> {
    pub async fn create(&self, activity_id: &str, event: RpcRuntimeEvent) -> Result<i32> {
        use schema::activity::dsl;
        use schema::runtime_event::dsl as dsl_event;

        let activity_id = activity_id.to_owned();

        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl_event::runtime_event)
                .values((
                    dsl_event::activity_id.eq(dsl::activity
                        .filter(dsl::natural_id.eq(&activity_id))
                        .first::<Activity>(conn)
                        .map_err(|e| match e {
                            diesel::NotFound => {
                                DaoError::NotFound(format!("activity {}", &activity_id))
                            }
                            e => e.into(),
                        })?
                        .id),
                    dsl_event::batch_id.eq(event.batch_id),
                    dsl_event::index.eq(event.index as i32),
                    dsl_event::timestamp.eq(event.timestamp),
                    dsl_event::type_id.eq(match &event.kind {
                        RuntimeEventKind::Started { command: _ } => RuntimeEventType::Started,
                        RuntimeEventKind::Finished {
                            return_code: _,
                            message: _,
                        } => RuntimeEventType::Finished,
                        RuntimeEventKind::StdOut(_) => RuntimeEventType::StdOut,
                        RuntimeEventKind::StdErr(_) => RuntimeEventType::StdErr,
                    }),
                    dsl_event::command.eq(match &event.kind {
                        RuntimeEventKind::Started { command } => {
                            Some(serde_json::to_string(&command)?)
                        }
                        _ => None,
                    }),
                    dsl_event::return_code.eq(match &event.kind {
                        RuntimeEventKind::Finished {
                            return_code,
                            message: _,
                        } => Some(return_code),
                        _ => None,
                    }),
                    dsl_event::message.eq(match &event.kind {
                        RuntimeEventKind::Finished {
                            return_code: _,
                            message,
                        } => message.as_ref(),
                        _ => None,
                    }),
                ))
                .execute(conn)?;

            let event_id = diesel::select(super::last_insert_rowid).first(conn)?;
            log::trace!("runtime event inserted: {}", event_id);

            Ok(event_id)
        })
        .await
    }
}
