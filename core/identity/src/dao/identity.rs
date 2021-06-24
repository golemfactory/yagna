pub use crate::db::models::Identity;
use crate::db::schema as s;
use diesel::prelude::*;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

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
        f: F,
    ) -> Result<R> {
        do_with_transaction(self.pool, f).await
    }

    pub async fn create_identity(&self, new_identity: Identity) -> Result<()> {
        let _rows = self
            .with_transaction(move |conn| {
                Ok(diesel::insert_into(s::identity::table)
                    .values(new_identity)
                    .execute(conn)?)
            })
            .await?;
        Ok(())
    }

    pub async fn update_keyfile(&self, identity_id: String, key_file_json: String) -> Result<()> {
        self.with_transaction(move |conn| {
            Ok(
                diesel::update(s::identity::table.filter(s::identity::identity_id.eq(identity_id)))
                    .set(s::identity::key_file_json.eq(&key_file_json))
                    .execute(conn)?,
            )
        })
        .await?;
        Ok(())
    }

    pub async fn list_identities(&self) -> Result<Vec<Identity>> {
        use crate::db::schema::identity::dsl::*;
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
        use crate::db::schema::identity::dsl::*;

        self.with_transaction(move |conn| {
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
