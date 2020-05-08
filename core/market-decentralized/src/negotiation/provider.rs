use std::sync::Arc;

use ya_persistence::executor::DbExecutor;

use crate::protocol::Negotiation;

/// Provider part of negotiation logic.
pub struct ProviderNegotiationEngine {
    db: DbExecutor,
    protocol: Arc<dyn Negotiation>,
}
