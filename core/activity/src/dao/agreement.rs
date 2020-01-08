use crate::dao::Result;
use diesel::prelude::*;
use ya_persistence::executor::ConnType;
use ya_persistence::models::Agreement;
use ya_persistence::schema;

pub struct AgreementDao<'c> {
    conn: &'c ConnType,
}

impl<'c> AgreementDao<'c> {
    pub fn new(conn: &'c ConnType) -> Self {
        Self { conn }
    }
}

impl<'c> AgreementDao<'c> {
    pub fn get(&self, agreement_id: &str) -> Result<Agreement> {
        use schema::agreement::dsl;

        dsl::agreement
            .filter(dsl::natural_id.eq(agreement_id))
            .first(self.conn)
    }
}
