use diesel::prelude::*;

use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

pub use crate::db::models::Identity;
use crate::db::schema as s;

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
    #[inline]
    async fn with_transaction<
        R: Send + 'static,
        F: FnOnce(&ConnType) -> Result<R> + Send + 'static,
    >(
        &self,
        label: &'static str,
        f: F,
    ) -> Result<R> {
        do_with_transaction(self.pool, label, f).await
    }

    pub async fn create_identity(&self, new_identity: Identity) -> Result<()> {
        let _rows = self
            .with_transaction("identity_dao_create_identity", move |conn| {
                Ok(diesel::insert_into(s::identity::table)
                    .values(new_identity)
                    .execute(conn)?)
            })
            .await?;
        Ok(())
    }

    pub async fn update_keyfile(&self, identity_id: String, key_file_json: String) -> Result<()> {
        self.with_transaction("identity_dao_update_keyfile", move |conn| {
            Ok(
                diesel::update(s::identity::table.filter(s::identity::identity_id.eq(identity_id)))
                    .set(s::identity::key_file_json.eq(&key_file_json))
                    .execute(conn)?,
            )
        })
        .await?;
        Ok(())
    }

    pub async fn mark_deleted(&self, identity_id: String) -> Result<()> {
        self.with_transaction("idenitiy::mark_deleted", move |conn| {
            Ok(
                diesel::update(s::identity::table.filter(s::identity::identity_id.eq(identity_id)))
                    .set(s::identity::is_deleted.eq(true))
                    .execute(conn)?,
            )
        })
        .await?;
        Ok(())
    }

    pub async fn list_identities(&self) -> Result<Vec<Identity>> {
        use crate::db::schema::identity::dsl::*;
        readonly_transaction(self.pool, "identity_dao_list_identities", |conn| {
            Ok(identity
                .filter(is_deleted.eq(false))
                .load::<Identity>(conn)?)
        })
        .await
    }

    pub async fn init_preconfigured(&self, preconfigured_identity: Identity) -> Result<Identity> {
        use crate::db::schema::identity::dsl as id_dsl;
        self.with_transaction("identity_dao_init_preconfigured", move |conn| {
            if let Some(id) = id_dsl::identity
                .filter(id_dsl::identity_id.eq(preconfigured_identity.identity_id))
                .get_result::<Identity>(conn)
                .optional()?
            {
                Ok(id)
            } else {
                diesel::insert_into(s::identity::table)
                    .values(&preconfigured_identity)
                    .execute(conn)?;
                diesel::update(s::identity::table)
                    .set(id_dsl::is_default.eq(false))
                    .filter(id_dsl::identity_id.ne(preconfigured_identity.identity_id))
                    .execute(conn)?;

                Ok(preconfigured_identity)
            }
        })
        .await
    }

    pub async fn init_default_key<KeyGenerator: Send + 'static + FnOnce() -> Result<Identity>>(
        &self,
        generator: KeyGenerator,
    ) -> Result<Identity> {
        use crate::db::schema::identity::dsl::*;

        self.with_transaction("identity_dao_init_default_key", move |conn| {
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
}
