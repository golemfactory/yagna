use diesel::prelude::*;
use diesel::sql_types::{Integer, Text, Timestamp};

use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::dao::Result;
use crate::db::{models::RuntimeEvent, models::RuntimeEventType, schema};

pub struct RuntimeEventDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for RuntimeEventDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        RuntimeEventDao { pool }
    }
}

impl<'c> RuntimeEventDao<'c> {
    pub async fn create(
        &self,
        activity_id: &str,
        event_type: RuntimeEventType,
    ) -> Result<i32> {
        // TODO
        Ok(42)
    }
}
