use crate::error::DbResult;
use crate::models::sync_notifs::{ReadObj, WriteObj};
use crate::schema::pay_sync_needed_notifs::dsl;
use chrono::NaiveDateTime;
use diesel::{self, QueryDsl, RunQueryDsl};

use ya_client_model::NodeId;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

pub struct SyncNotifsDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for SyncNotifsDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> SyncNotifsDao<'c> {
    /// Creates a new sync notif tracking entry for given node-id
    pub async fn upsert(&self, peer_id: NodeId) -> DbResult<()> {
        let sync_notif = WriteObj::new(peer_id);
        do_with_transaction(self.pool, "sync_notifs_dao_upsert", move |conn| {
            diesel::delete(dsl::pay_sync_needed_notifs.find(peer_id))
                .execute(conn)
                .ok();

            diesel::insert_into(dsl::pay_sync_needed_notifs)
                .values(sync_notif)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    /// Bump retry by 1 and update timestamp
    pub async fn increment_retry(&self, peer_id: NodeId, ts: NaiveDateTime) -> DbResult<()> {
        do_with_transaction(self.pool, "sync_notifs_dao_increment_retry", move |conn| {
            let mut read: ReadObj = dsl::pay_sync_needed_notifs.find(peer_id).first(conn)?;
            read.retries += 1;
            read.last_ping = ts;

            diesel::update(dsl::pay_sync_needed_notifs.find(peer_id))
                .set(WriteObj::from_read(read))
                .execute(conn)?;

            Ok(())
        })
        .await
    }

    /// Remove entry
    pub async fn drop(&self, peer_id: NodeId) -> DbResult<()> {
        do_with_transaction(self.pool, "sync_notifs_dao_drop", move |conn| {
            diesel::delete(dsl::pay_sync_needed_notifs.find(peer_id)).execute(conn)?;
            Ok(())
        })
        .await
    }

    /// List all planned syncs
    pub async fn list(&self) -> DbResult<Vec<ReadObj>> {
        readonly_transaction(self.pool, "sync_notifs_dao_list", move |conn| {
            let sync_notif = dsl::pay_sync_needed_notifs.load(conn)?;

            Ok(sync_notif)
        })
        .await
    }
}
