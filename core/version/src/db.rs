pub(crate) mod dao {
    use anyhow::Result;
    use chrono::NaiveDateTime;
    use diesel::dsl::{exists, select};
    use diesel::prelude::*;
    use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

    use crate::db::model::Release as DBRelease;
    use crate::db::schema::version_release::dsl as release;
    use crate::db::schema::version_release::dsl::version_release;

    pub struct ReleaseDAO<'c> {
        pool: &'c PoolType,
    }
    impl<'a> AsDao<'a> for ReleaseDAO<'a> {
        fn as_dao(pool: &'a PoolType) -> Self {
            Self { pool }
        }
    }

    impl<'c> ReleaseDAO<'c> {
        pub async fn new_release(&self, r: self_update::update::Release) -> Result<()> {
            let db_release = DBRelease {
                version: r.version,
                name: r.name,
                seen: false,
                release_ts: NaiveDateTime::parse_from_str(&r.date, "%Y-%m-%dT%H:%M:%S%Z")?,
                insertion_ts: None,
                update_ts: None,
            };
            Ok(do_with_transaction(self.pool, move |conn| {
                if !select(exists(
                    version_release.filter(release::version.eq(&db_release.version)),
                ))
                .get_result(conn)?
                {
                    diesel::insert_into(version_release)
                        .values(&db_release)
                        .execute(conn)?;
                };
                Result::<()>::Ok(())
            })
            .await?)
        }

        /*
        pub async fn clean(&self) {
            // TODO
        }
        */

        pub async fn pending_release(&self) -> Result<Option<DBRelease>> {
            readonly_transaction(self.pool, move |conn| {
                let query = version_release
                    .filter(release::seen.eq(false))
                    .order(release::release_ts.desc())
                    .into_boxed();

                Ok(query.first::<DBRelease>(conn).optional()?)
            })
            .await
        }

        pub async fn current_release(&self) -> Result<Option<DBRelease>> {
            readonly_transaction(self.pool, move |conn| {
                Ok(version_release
                    .filter(release::version.eq(ya_compile_time_utils::semver_str()))
                    .first::<DBRelease>(conn)
                    .optional()?)
            })
            .await
        }
    }
}
pub(crate) mod model {
    use crate::db::schema::version_release;
    use chrono::NaiveDateTime;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Identifiable, Insertable, Queryable, Serialize, Deserialize)]
    #[primary_key(version)]
    #[table_name = "version_release"]
    pub struct Release {
        pub version: String,
        pub name: String,
        pub seen: bool,
        pub release_ts: NaiveDateTime,
        pub insertion_ts: Option<NaiveDateTime>,
        pub update_ts: Option<NaiveDateTime>,
    }

    impl std::fmt::Display for Release {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{} {} released {}",
                self.version, self.name, self.release_ts
            )
        }
    }
}
pub(crate) mod schema;
