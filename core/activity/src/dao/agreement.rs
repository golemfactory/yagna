use diesel::prelude::*;

use ya_persistence::executor::ConnType;
use ya_persistence::models::{Agreement, NewAgreement};
use ya_persistence::schema::agreement::dsl;

use crate::dao::Result;

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
        dsl::agreement
            .filter(dsl::natural_id.eq(agreement_id))
            .first(self.conn)
    }

    pub fn create(&self, new_agreement: NewAgreement) -> Result<()> {
        diesel::insert_into(dsl::agreement)
            .values((&new_agreement,))
            .execute(self.conn)
            .map(|_| ())
    }
}
