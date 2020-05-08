use std::sync::Arc;

use crate::matcher::Matcher;
use crate::negotiation::{ProviderNegotiationEngine, RequestorNegotiationEngine};

/// Structure connecting all market objects.
pub struct Market {
    matcher: Arc<Matcher>,
    provider_negotiation_engine: Arc<ProviderNegotiationEngine>,
    requestor_negotiation_engine: Arc<RequestorNegotiationEngine>,
}
