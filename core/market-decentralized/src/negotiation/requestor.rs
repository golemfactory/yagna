use std::sync::Arc;

use ya_persistence::executor::DbExecutor;

use crate::protocol::Negotiation;

/// Requestor part of negotiation logic.
pub struct RequestorNegotiationEngine {
    db: DbExecutor,
    protocol: Arc<dyn Negotiation>,
}
