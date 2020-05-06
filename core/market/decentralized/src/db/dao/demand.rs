use ya_persistence::executor::{do_with_transaction, readonly_transaction, AsDao, PoolType};


#[allow(unused)]
pub struct DemandDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for DemandDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

