use ya_agreement_utils::OfferDefinition;

use crate::market::negotiator::factory::DemandValidationNegotiatorConfig;
use crate::market::negotiator::{
    AgreementResult, NegotiationResult, NegotiatorComponent, ProposalView,
};

/// Negotiator that verifies that all required fields are present in proposal.
pub struct DemandValidation {
    required_fields: Vec<String>,
}

impl DemandValidation {
    pub fn new(config: &DemandValidationNegotiatorConfig) -> DemandValidation {
        DemandValidation {
            required_fields: config
                .required_fields
                .iter()
                .map(|x| x.to_string())
                .collect(),
        }
    }
}

impl NegotiatorComponent for DemandValidation {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        let missing_fields = self
            .required_fields
            .iter()
            .cloned()
            .filter(|x| demand.pointer(x).is_none())
            .collect::<Vec<String>>();
        if missing_fields.is_empty() {
            Ok(NegotiationResult::Ready { offer })
        } else {
            log::info!(
                "'DemandValidation' negotiator: Reject proposal [{}] due to missing fields: {}",
                demand.id,
                missing_fields.join(",")
            );
            Ok(NegotiationResult::Reject {
                message: format!("Missing fields: {}", missing_fields.join(",")),
                is_final: false,
            })
        }
    }

    fn fill_template(
        &mut self,
        offer_template: OfferDefinition,
    ) -> anyhow::Result<OfferDefinition> {
        Ok(offer_template)
    }

    fn on_agreement_terminated(
        &mut self,
        _agreement_id: &str,
        _result: &AgreementResult,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn on_agreement_approved(&mut self, _agreement_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
