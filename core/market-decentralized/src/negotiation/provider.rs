use std::sync::Arc;

use ya_persistence::executor::DbExecutor;

use super::errors::NegotiationError;

/// Provider part of negotiation logic.
/// TODO: Too long name.
pub struct ProviderNegotiationEngine {
    db: DbExecutor,
}

impl ProviderNegotiationEngine {
    pub fn new(db: DbExecutor) -> Result<Arc<ProviderNegotiationEngine>, NegotiationError> {
        Ok(Arc::new(ProviderNegotiationEngine { db }))
    }
}
