use ya_core_model::activity::local::Credentials;
use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::dao::Result;
use crate::db::{models::ActivityCredentials, schema};
use diesel::{OptionalExtension, QueryDsl, RunQueryDsl};

pub struct ActivityCredentialsDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for ActivityCredentialsDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        ActivityCredentialsDao { pool }
    }
}

impl<'c> ActivityCredentialsDao<'c> {
    pub async fn get(&self, activity_id: &str) -> Result<Option<ActivityCredentials>> {
        use schema::activity_credentials::dsl as dsl_cred;
        let activity_id = activity_id.to_owned();

        readonly_transaction(self.pool, "activity_credentials_get", move |conn| {
            Ok(dsl_cred::activity_credentials
                .find(&activity_id)
                .first::<ActivityCredentials>(conn)
                .optional()?)
        })
        .await
    }

    pub async fn set(&self, activity_id: &str, credentials: Credentials) -> Result<()> {
        use schema::activity_credentials::dsl as dsl_cred;

        let activity_id = activity_id.to_owned();
        let cred = ActivityCredentials {
            activity_id: activity_id.clone(),
            credentials: serde_json::to_string(&credentials)?,
        };

        do_with_transaction(self.pool, "activity_credentials_set", move |conn| {
            if let Err(e) = diesel::insert_into(dsl_cred::activity_credentials)
                .values(&cred)
                .execute(conn)
            {
                use diesel::result::{DatabaseErrorKind, Error};
                match e {
                    Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                        diesel::update(dsl_cred::activity_credentials.find(&activity_id))
                            .set(&cred)
                            .execute(conn)?;
                    }
                    _ => return Err(e.into()),
                }
            }
            Ok(())
        })
        .await
    }
}
