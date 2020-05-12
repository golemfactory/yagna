use std::sync::Arc;

use ya_persistence::executor::DbExecutor;

use super::errors::{NegotiationError, NegotiationInitError};

/// Provider part of negotiation logic.
/// TODO: Too long name.
pub struct ProviderNegotiationEngine {
    db: DbExecutor,
}

impl ProviderNegotiationEngine {
    pub fn new(db: DbExecutor) -> Result<Arc<ProviderNegotiationEngine>, NegotiationInitError> {
        Ok(Arc::new(ProviderNegotiationEngine { db }))
    }

    pub async fn bind_gsb(&self) -> Result<(), NegotiationInitError> {
        Ok(())
    }
}
