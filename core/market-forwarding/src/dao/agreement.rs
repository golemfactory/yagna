use diesel::prelude::*;

use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};

use crate::db::models::{Agreement, NewAgreement};
use crate::db::schema::agreement::dsl;

use crate::dao::Result;

pub struct AgreementDao<'c> {
    pool: &'c PoolType,
}

impl<'a> AsDao<'a> for AgreementDao<'a> {
    fn as_dao(pool: &'a PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> AgreementDao<'c> {
    pub async fn get(&self, agreement_id: String) -> Result<Agreement> {
        readonly_transaction(self.pool, move |conn| {
            Ok(dsl::agreement
                .filter(dsl::natural_id.eq(agreement_id))
                .first(conn)?)
        })
        .await
    }

    pub async fn create(&self, new_agreement: NewAgreement) -> Result<()> {
        do_with_transaction(self.pool, move |conn| {
            Ok(diesel::insert_into(dsl::agreement)
                .values((&new_agreement,))
                .execute(conn)
                .map(|_| ())?)
        })
        .await
    }
}
