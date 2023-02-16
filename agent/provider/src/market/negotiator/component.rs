use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    fn fill_template(&mut self, offer_template: OfferDefinition)
        -> anyhow::Result<OfferDefinition>;

    /// Called when Agreement was finished. `NegotiatorComponent` can use termination
    /// result to adjust his future negotiation strategy.
    fn on_agreement_terminated(
        &mut self,
        agreement_id: &str,
        result: &AgreementResult,
    ) -> anyhow::Result<()>;

    /// Called when Negotiator decided to approve Agreement. It's only notification,
    /// `NegotiatorComponent` can't reject Agreement anymore.
    fn on_agreement_approved(&mut self, agreement_id: &str) -> anyhow::Result<()>;
}

#[derive(Default)]
pub struct NegotiatorsPack {
    components: HashMap<String, Box<dyn NegotiatorComponent>>,
}

impl NegotiatorsPack {
    pub fn add_component(
        mut self,
        name: &str,
        component: Box<dyn NegotiatorComponent>,
    ) -> NegotiatorsPack {
        self.components.insert(name.to_string(), component);
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
