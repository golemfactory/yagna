use anyhow::anyhow;
use serde::{Deserialize, Serialize};

pub use ya_agreement_utils::{OfferDefinition, ProposalView};

use crate::market::negotiator::AgreementResult;

/// Result returned by `NegotiatorComponent` during Proposals evaluation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NegotiationResult {
    /// `NegotiatorComponent` fully negotiated his part of Proposal,
    /// and it can be turned into valid Agreement. Provider will send
    /// counter Proposal.
    Ready { offer: ProposalView },
    /// Proposal is not ready to become Agreement, but negotiations
    /// are in progress.
    Negotiating { offer: ProposalView },
    /// Proposal is not acceptable and should be rejected.
    /// Negotiations can't be continued.
    Reject { message: String, is_final: bool },
}

/// `NegotiatorComponent` implements negotiation logic for part of Agreement
/// specification. Components should be as granular as possible to allow composition
/// with other Components.
///
/// Future goal is to allow developers to create their own specifications and implement
/// components, that are able to negotiate this specification.
/// It would be useful to have `NegotiatorComponent`, that can be loaded from shared library
/// or can communicate with negotiation logic in external process (maybe RPC or TCP??).
pub trait NegotiatorComponent {
    /// Push forward negotiations as far as you can.
    /// `NegotiatorComponent` should modify only properties in his responsibility
    /// and return remaining part of Proposal unchanged.
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult>;

    /// Called during Offer creation. `NegotiatorComponent` should add properties
    /// and constraints for which it is responsible during future negotiations.
    fn fill_template(
        &mut self,
        offer_template: OfferDefinition,
    ) -> anyhow::Result<OfferDefinition> {
        Ok(offer_template)
    }

    /// Called when Agreement was finished. `NegotiatorComponent` can use termination
    /// result to adjust his future negotiation strategy.
    fn on_agreement_terminated(
        &mut self,
        _agreement_id: &str,
        _result: &AgreementResult,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when Negotiator decided to approve Agreement. It's only notification,
    /// `NegotiatorComponent` can't reject Agreement anymore.
    fn on_agreement_approved(&mut self, _agreement_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Components are consulted in the order they were added: `negotiate_step`
/// short-circuits on the first `Reject`, so the order decides which component
/// speaks for the Provider (e.g. `RejectOnShutdown` is added first, so a
/// shutting-down Provider answers with a final "shutting down" rejection
/// instead of whatever another component would say).
#[derive(Default)]
pub struct NegotiatorsPack {
    components: Vec<(String, Box<dyn NegotiatorComponent>)>,
}

impl NegotiatorsPack {
    pub fn add_component(
        mut self,
        name: &str,
        component: Box<dyn NegotiatorComponent>,
    ) -> NegotiatorsPack {
        self.components.push((name.to_string(), component));
        self
    }
}

impl NegotiatorComponent for NegotiatorsPack {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        mut offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        let mut all_ready = true;
        for (name, component) in &mut self.components {
            let result = component.negotiate_step(demand, offer)?;
            offer = match result {
                NegotiationResult::Ready { offer } => offer,
                NegotiationResult::Negotiating { offer } => {
                    log::info!(
                        "Negotiator component '{name}' is still negotiating Proposal [{}].",
                        demand.id
                    );
                    all_ready = false;
                    offer
                }
                NegotiationResult::Reject { message, is_final } => {
                    return Ok(NegotiationResult::Reject { message, is_final })
                }
            }
        }

        // Full negotiations is ready only, if all `NegotiatorComponent` returned
        // ready state. Otherwise we must still continue negotiations.
        Ok(match all_ready {
            true => NegotiationResult::Ready { offer },
            false => NegotiationResult::Negotiating { offer },
        })
    }

    fn fill_template(
        &mut self,
        mut offer_template: OfferDefinition,
    ) -> anyhow::Result<OfferDefinition> {
        for (name, component) in &mut self.components {
            offer_template = component.fill_template(offer_template).map_err(|e| {
                anyhow!(
                    "Negotiator component '{}' failed filling Offer template. {}",
                    name,
                    e
                )
            })?;
        }
        Ok(offer_template)
    }

    fn on_agreement_terminated(
        &mut self,
        agreement_id: &str,
        result: &AgreementResult,
    ) -> anyhow::Result<()> {
        for (name, component) in &mut self.components {
            component
                .on_agreement_terminated(agreement_id, result)
                .map_err(|e| {
                    log::warn!(
                        "Negotiator component '{}' failed handling Agreement [{}] termination. {}",
                        name,
                        agreement_id,
                        e
                    )
                })
                .ok();
        }
        Ok(())
    }

    fn on_agreement_approved(&mut self, agreement_id: &str) -> anyhow::Result<()> {
        for (name, component) in &mut self.components {
            component
                .on_agreement_approved(agreement_id)
                .map_err(|e| {
                    log::warn!(
                        "Negotiator component '{}' failed handling Agreement [{}] approval. {}",
                        name,
                        agreement_id,
                        e
                    )
                })
                .ok();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use ya_agreement_utils::agreement::expand;
    use ya_agreement_utils::OfferTemplate;
    use ya_client_model::market::proposal::State;

    struct Rejecting(&'static str);

    impl NegotiatorComponent for Rejecting {
        fn negotiate_step(
            &mut self,
            _demand: &ProposalView,
            _offer: ProposalView,
        ) -> anyhow::Result<NegotiationResult> {
            Ok(NegotiationResult::Reject {
                message: self.0.to_string(),
                is_final: true,
            })
        }
    }

    fn proposal() -> ProposalView {
        ProposalView {
            content: OfferTemplate {
                properties: expand(json!({})),
                constraints: "()".to_string(),
            },
            id: "proposalId".to_string(),
            issuer: Default::default(),
            state: State::Initial,
            timestamp: Utc::now(),
        }
    }

    /// `negotiate_step` short-circuits on the first `Reject`, so the first
    /// added component must be the first one consulted - insertion order,
    /// not map order.
    #[test]
    fn components_are_consulted_in_insertion_order() {
        let mut pack = NegotiatorsPack::default()
            .add_component("first", Box::new(Rejecting("first")))
            .add_component("second", Box::new(Rejecting("second")));

        match pack.negotiate_step(&proposal(), proposal()).unwrap() {
            NegotiationResult::Reject { message, .. } => assert_eq!(message, "first"),
            other => panic!("expected Reject, got {:?}", other),
        }
    }
}
