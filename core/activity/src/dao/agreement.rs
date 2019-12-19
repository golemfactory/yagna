use crate::dao::Result;
use crate::db::ConnType;
use diesel::prelude::*;

#[derive(Clone, Debug)]
pub struct Agreement {
    pub id: String,
    pub requestor_id: String,
    pub provider_id: String,
}

pub struct AgreementDao<'c> {
    conn: &'c ConnType,
}

impl<'c> AgreementDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> AgreementDao<'c> {
    #[allow(dead_code)]
    pub fn create(&self, agreement_id: &str, requestor_id: &str, provider_id: &str) -> Result<()> {
        use crate::db::schema::agreements::dsl;

        diesel::insert_into(dsl::agreements)
            .values((
                dsl::id.eq(agreement_id),
                dsl::requestor_id.eq(requestor_id),
                dsl::provider_id.eq(provider_id),
            ))
            .execute(self.conn)
            .map(|_| ())
    }

    pub fn get(&self, agreement_id: &str) -> Result<Agreement> {
        use crate::db::schema::agreements::dsl;

        dsl::agreements
            .filter(dsl::id.eq(agreement_id))
            .select((dsl::id, dsl::requestor_id, dsl::provider_id))
            .first(self.conn)
            .map(|(id, requestor_id, provider_id)| Agreement {
                id,
                requestor_id,
                provider_id,
            })
    }
}
