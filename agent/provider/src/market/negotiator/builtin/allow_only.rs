use crate::market::negotiator::{NegotiationResult, NegotiatorComponent};
use crate::provider_agent::AgentNegotiatorsConfig;
use crate::rules::restrict::AllowOnlyValidator;
use crate::rules::{CheckRulesResult, RulesManager};

use ya_agreement_utils::ProposalView;
use ya_manifest_utils::DEMAND_MANIFEST_NODE_DESCRIPTOR_PROPERTY;

pub struct AllowOnly {
    rules: RulesManager,
}

impl AllowOnly {
    pub fn new(agent_negotiators_cfg: AgentNegotiatorsConfig) -> Self {
        Self {
            rules: agent_negotiators_cfg.rules_manager,
        }
    }
}

impl NegotiatorComponent for AllowOnly {
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
            .allow_only()
            .check_allow_only_rule(demand.issuer, node_descriptor)
        {
            CheckRulesResult::Accept => Ok(NegotiationResult::Ready { offer }),
            CheckRulesResult::Reject(reason) => {
                log::debug!(
                    "[AllowOnly] Rejecting Proposal from Requestor {}, reason: {reason}",
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
