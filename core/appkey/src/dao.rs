use crate::db::models::{AppKey, Role};
use chrono::Local;
use diesel::prelude::*;
use diesel::{Connection, ExpressionMethods, RunQueryDsl};
use std::cmp::max;
use ya_persistence::executor::ConnType;

pub type Result<T> = std::result::Result<T, crate::error::Error>;

pub struct AppKeyDao<'c> {
    conn: &'c ConnType,
}

impl<'c> AppKeyDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> AppKeyDao<'c> {
    pub fn create(&self, key: String, name: String, role: String, identity: String) -> Result<()> {
        use crate::db::schema::app_key as app_key_dsl;
        use crate::db::schema::role as role_dsl;

        self.conn.transaction(|| {
            let role: Role = role_dsl::table
                .filter(role_dsl::name.eq(role))
                .first(self.conn)?;

            diesel::insert_into(app_key_dsl::table)
                .values((
                    app_key_dsl::role_id.eq(&role.id),
                    app_key_dsl::name.eq(name),
                    app_key_dsl::key.eq(key),
                    app_key_dsl::identity.eq(identity),
                    app_key_dsl::created_date.eq(Local::now().naive_local()),
                ))
                .execute(self.conn)?;

            Ok(())
        })
    }

    pub fn get(&self, key: String) -> Result<(AppKey, Role)> {
        use crate::db::schema::app_key as app_key_dsl;
        use crate::db::schema::role as role_dsl;

        self.conn.transaction(|| {
            let result = app_key_dsl::table
                .inner_join(role_dsl::table)
                .filter(app_key_dsl::key.eq(key))
                .first(self.conn)?;

            Ok(result)
        })
    }

    pub fn list(
        &self,
        identity: Option<String>,
        page: u32,
        per_page: u32,
    ) -> Result<(Vec<(AppKey, Role)>, u32)> {
        use crate::db::schema::app_key as app_key_dsl;
        use crate::db::schema::role as role_dsl;

        let offset = max(0, (page - 1) * per_page);
        self.conn.transaction(|| {
            let query = app_key_dsl::table
                .inner_join(role_dsl::table)
                .limit(per_page as i64)
                .offset(offset as i64);

            let results: Vec<(AppKey, Role)> = if let Some(id) = identity {
                query.filter(app_key_dsl::identity.eq(id)).load(self.conn)
            } else {
                query.load(self.conn)
            }?;

            // TODO: use DB INSERT / DELETE triggers and internal counters in place of count
            let total: i64 = app_key_dsl::table
                .select(diesel::expression::dsl::count(app_key_dsl::id))
                .first(self.conn)?;
            let pages = (total as f64 / per_page as f64).ceil() as u32;

            Ok((results, pages))
        })
    }

    pub fn remove(&self, name: String, identity: Option<String>) -> Result<()> {
        use crate::db::schema::app_key as app_key_dsl;

        self.conn.transaction(|| {
            let filter = app_key_dsl::table.filter(app_key_dsl::name.eq(name.as_str()));
            if let Some(id) = identity {
                diesel::delete(filter.filter(app_key_dsl::identity.eq(id.as_str())))
                    .execute(self.conn)
            } else {
                diesel::delete(filter).execute(self.conn)
            }?;

            Ok(())
        })
    }
}
