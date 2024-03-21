use crate::market::negotiator::{NegotiationResult, NegotiatorComponent};
use crate::provider_agent::AgentNegotiatorsConfig;
use crate::rules::restrict::BlacklistValidator;
use crate::rules::{CheckRulesResult, RulesManager};

use ya_agreement_utils::ProposalView;
use ya_manifest_utils::DEMAND_MANIFEST_NODE_DESCRIPTOR_PROPERTY;

pub struct Blacklist {
    rules: RulesManager,
}

impl Blacklist {
    pub fn new(agent_negotiators_cfg: AgentNegotiatorsConfig) -> Self {
        Self {
            rules: agent_negotiators_cfg.rules_manager,
        }
    }
}

impl NegotiatorComponent for Blacklist {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        let node_descriptor = demand
            .get_property::<serde_json::Value>(DEMAND_MANIFEST_NODE_DESCRIPTOR_PROPERTY)
            .ok();

        match self
            .rules
            .blacklist()
            .check_blacklist_rule(demand.issuer, node_descriptor)
        {
            CheckRulesResult::Accept => Ok(NegotiationResult::Ready { offer }),
            CheckRulesResult::Reject(reason) => {
                log::debug!(
                    "[Blacklist] Rejecting Proposal from Requestor {}, reason: {reason}",
                    demand.issuer
                );
                Ok(NegotiationResult::Reject {
                    message: reason,
                    is_final: true,
                })
            }
        }
    }
}
