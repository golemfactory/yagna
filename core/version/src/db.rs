pub(crate) mod dao {
    use anyhow::Result;
    use chrono::NaiveDateTime;
    use diesel::dsl::{exists, select};
    use diesel::prelude::*;
    use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

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
                release_ts: NaiveDateTime::parse_from_str(&r.date, "%Y-%m-%d %H:%M:%S")?,
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
            do_with_transaction(self.pool, move |conn| {
                let query = version_release
                    .filter(release::seen.eq(false))
                    .order(release::release_ts.desc())
                    .into_boxed();

                Ok(query.first::<DBRelease>(conn).optional()?)
            })
            .await
        }

        pub async fn skip_pending_release(&self) -> Result<Option<DBRelease>> {
            let mut pending_release = match self.pending_release().await? {
                Some(r) => r,
                None => return Ok(None),
            };

            do_with_transaction(self.pool, move |conn| {
                let num_updated = diesel::update(version_release.find(&pending_release.version))
                    .set(release::seen.eq(true))
                    .execute(conn)?;
                pending_release.seen = true;
                match num_updated {
                    0 => anyhow::bail!("no release updated: {}", pending_release),
                    1 => Ok(Some(pending_release)),
                    _ => anyhow::bail!("more than one release updated: {}", pending_release),
                }
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

    impl From<Release> for ya_core_model::version::Release {
        fn from(r: Release) -> Self {
            Self {
                version: r.version,
                name: r.name,
                seen: r.seen,
                release_ts: r.release_ts,
                insertion_ts: r.insertion_ts,
                update_ts: r.update_ts,
            }
        }
    }
}
pub(crate) mod schema;
