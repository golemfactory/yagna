use diesel::prelude::*;

use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};

use crate::db::models::Agreement;
use crate::db::schema::market_agreement::dsl;
use crate::db::DbResult;

pub struct AgreementDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for AgreementDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> AgreementDao<'c> {
    pub async fn save(&self, agreement: Agreement) -> DbResult<()> {
        do_with_transaction(self.pool, move |conn| {
            diesel::insert_into(dsl::market_agreement)
                .values(&agreement)
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
