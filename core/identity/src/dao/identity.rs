pub use crate::db::models::Identity;
use crate::db::schema as s;
use crate::db::schema::identity::dsl::*;
use diesel::prelude::*;

use ya_client_model::NodeId;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

type Result<T> = std::result::Result<T, super::Error>;

pub struct IdentityDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for IdentityDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> IdentityDao<'c> {
    pub async fn create_identity(&self, new_identity: Identity) -> Result<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(s::identity::table)
                .values(new_identity)
                .execute(conn)?;
            Ok(())
        })
        .await
    }

    pub async fn list_identities(&self) -> Result<Vec<Identity>> {
        readonly_transaction(self.pool, |conn| {
            Ok(identity
                .filter(is_deleted.eq(false))
                .load::<Identity>(conn)?)
        })
        .await
    }

    pub async fn init_default_key<KeyGenerator: Send + 'static + FnOnce() -> Result<Identity>>(
        &self,
        generator: KeyGenerator,
    ) -> Result<Identity> {
        do_with_transaction(self.pool, move |conn| {
            if let Some(id) = identity
                .filter(is_default.eq(true))
                .get_result::<Identity>(conn)
                .optional()?
            {
                return Ok(id);
            }
            let new_identity = generator()?;
            diesel::insert_into(s::identity::table)
                .values(&new_identity)
                .execute(conn)?;

            Ok(new_identity)
        })
        .await
    }

    pub async fn update_identity(
        &self,
        node_id: NodeId,
        update_alias: Option<String>,
        set_default: bool,
        prev_default: NodeId,
    ) -> Result<()> {
        do_with_transaction(self.pool, move |conn| {
            if update_alias.is_some() {
                let _ = diesel::update(identity.filter(identity_id.eq(&node_id)))
                    .set(alias.eq(&update_alias.unwrap()))
                    .execute(conn)?;
            }
            if set_default && prev_default != node_id {
                diesel::update(identity.filter(identity_id.eq(&prev_default)))
                    .set(is_default.eq(false))
                    .execute(conn)?;
                diesel::update(identity.filter(identity_id.eq(&node_id)))
                    .set(is_default.eq(true))
                    .execute(conn)?;
            }
            Ok(())
        })
        .await
    }

    pub async fn mark_deleted(&self, node_id: NodeId) -> Result<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::update(identity.filter(identity_id.eq(&node_id)))
                .set(is_deleted.eq(true))
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
