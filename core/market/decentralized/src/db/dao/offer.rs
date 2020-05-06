use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};


#[allow(unused)]
pub struct OfferDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for OfferDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

impl<'c> OfferDao<'c> {
    //pub async fn insert_offer()
}

