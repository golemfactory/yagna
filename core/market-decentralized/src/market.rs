use std::sync::Arc;

use crate::negotiation::{RequestorNegotiationEngine, ProviderNegotiationEngine};
use crate::matcher::Matcher;


/// Structure connecting all market objects.
pub struct Market {
    matcher: Arc<Matcher>,
    provider_negotiation_engine: Arc<ProviderNegotiationEngine>,
    requestor_negotiation_engine: Arc<RequestorNegotiationEngine>,
}

