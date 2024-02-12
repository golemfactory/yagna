pub use crate::dao::Error as DaoError;
pub use crate::db::models::{AppKey, Role};
use chrono::Utc;
use diesel::prelude::*;

use diesel::{ExpressionMethods, RunQueryDsl};
use std::cmp::max;
use ya_client_model::NodeId;
use ya_persistence::executor::{
    do_with_transaction, readonly_transaction, AsDao, ConnType, PoolType,
};

pub type Result<T> = std::result::Result<T, DaoError>;

pub struct AppKeyDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for AppKeyDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        AppKeyDao { pool }
    }
}

impl<'c> AppKeyDao<'c> {
    pub async fn with_connection<R: Send + 'static, F>(&self, f: F) -> Result<R>
    where
        F: Send + 'static + FnOnce(&ConnType) -> Result<R>,
    {
        readonly_transaction(self.pool, "app_key_dao_with_connection", f).await
    }

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

    pub async fn create(
        &self,
        key: String,
        name: String,
        role: String,
        identity: NodeId,
        cors_allow_origin: Vec<String>,
    ) -> Result<()> {
        use crate::db::schema::app_key as app_key_dsl;
        use crate::db::schema::role as role_dsl;

        let cors_allow_origin =
            Some(serde_json::to_string(&cors_allow_origin).unwrap_or_else(|_| "[]".to_string()));

        do_with_transaction(self.pool, "app_key_dao_create", move |conn| {
            let role: Role = role_dsl::table
                .filter(role_dsl::name.eq(role))
                .first(conn)?;

            diesel::insert_into(app_key_dsl::table)
                .values((
                    app_key_dsl::role_id.eq(&role.id),
                    app_key_dsl::name.eq(name),
                    app_key_dsl::key.eq(key),
                    app_key_dsl::identity_id.eq(identity),
                    app_key_dsl::created_date.eq(Utc::now().naive_utc()),
                    app_key_dsl::allow_origins.eq(cors_allow_origin),
                ))
                .execute(conn)?;

            Ok(())
        })
        .await
    }

    pub async fn get(&self, key: String) -> Result<(AppKey, Role)> {
        use crate::db::schema::app_key as app_key_dsl;
        use crate::db::schema::role as role_dsl;

        readonly_transaction(self.pool, "app_key_dao_get", move |conn| {
            let result = app_key_dsl::table
                .inner_join(role_dsl::table)
                .filter(app_key_dsl::key.eq(key))
                .first(conn)?;

            Ok(result)
        })
        .await
    }

    pub async fn get_for_id(&self, identity_id: String) -> Result<(AppKey, Role)> {
        use crate::db::schema::app_key as app_key_dsl;
        use crate::db::schema::role as role_dsl;

        readonly_transaction(self.pool, "app_key_dao_get_for_id", |conn| {
            let result = app_key_dsl::table
                .inner_join(role_dsl::table)
                .filter(app_key_dsl::identity_id.eq(identity_id))
                .first(conn)?;

            Ok(result)
        })
        .await
    }

    pub async fn get_for_name(&self, name: String) -> Result<(AppKey, Role)> {
        use crate::db::schema::app_key as app_key_dsl;
        use crate::db::schema::role as role_dsl;

        readonly_transaction(self.pool, "app_key_dao_get_for_name", |conn| {
            let result = app_key_dsl::table
                .inner_join(role_dsl::table)
                .filter(app_key_dsl::name.eq(name))
                .first(conn)?;

            Ok(result)
        })
        .await
    }

    pub async fn list(
        &self,
        identity: Option<String>,
        page: u32,
        per_page: u32,
    ) -> Result<(Vec<(AppKey, Role)>, u32)> {
        use crate::db::schema::app_key as app_key_dsl;
        use crate::db::schema::role as role_dsl;

        let offset = max(0, (page - 1) * per_page);
        readonly_transaction(self.pool, "app_key_dao_list", move |conn| {
            let query = app_key_dsl::table
                .inner_join(role_dsl::table)
                .limit(per_page as i64)
                .offset(offset as i64);

            let results: Vec<(AppKey, Role)> = if let Some(id) = identity {
                query.filter(app_key_dsl::identity_id.eq(id)).load(conn)
            } else {
                query.load(conn)
            }?;

            // TODO: use DB INSERT / DELETE triggers and internal counters in place of count
            let total: i64 = app_key_dsl::table
                .select(diesel::expression::dsl::count(app_key_dsl::id))
                .first(conn)?;
            let pages = (total as f64 / per_page as f64).ceil() as u32;

            Ok((results, pages))
        })
        .await
    }

    pub async fn remove(&self, name: String, identity: Option<String>) -> Result<()> {
        use crate::db::schema::app_key as app_key_dsl;

        self.with_transaction("app_key_dao_remove", move |conn| {
            let filter = app_key_dsl::table.filter(app_key_dsl::name.eq(name.as_str()));
            if let Some(id) = identity {
                diesel::delete(filter.filter(app_key_dsl::identity_id.eq(id.as_str())))
                    .execute(conn)
            } else {
                diesel::delete(filter).execute(conn)
            }?;

            Ok(())
        })
        .await
    }
}
