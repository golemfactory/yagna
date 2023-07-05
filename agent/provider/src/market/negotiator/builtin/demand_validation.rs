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

#[cfg(test)]
mod test_demand_validation_negotiator {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use ya_agreement_utils::agreement::expand;
    use ya_agreement_utils::{InfNodeInfo, NodeInfo, OfferTemplate, ServiceInfo};
    use ya_client_model::market::proposal::State;

    fn config() -> DemandValidationNegotiatorConfig {
        DemandValidationNegotiatorConfig {
            required_fields: vec![
                "/golem/com/freebies".to_string(),
                "/golem/com/payment/address".to_string(),
            ],
        }
    }

    fn properties_to_proposal(value: serde_json::Value) -> ProposalView {
        ProposalView {
            content: OfferTemplate {
                properties: expand(value),
                constraints: "()".to_string(),
            },
            id: "proposalId".to_string(),
            issuer: Default::default(),
            state: State::Initial,
            timestamp: Utc::now(),
        }
    }

    fn example_offer() -> OfferDefinition {
        OfferDefinition {
            node_info: NodeInfo::with_name("nodeInfoName"),
            srv_info: ServiceInfo::new(InfNodeInfo::default(), serde_json::Value::Null),
            com_info: Default::default(),
            offer: OfferTemplate::default(),
        }
    }

    trait ToProposal {
        fn to_proposal(self) -> ProposalView;
    }

    impl ToProposal for OfferDefinition {
        fn to_proposal(self) -> ProposalView {
            let template = self.into_template();
            ProposalView {
                content: OfferTemplate {
                    properties: expand(template.properties),
                    constraints: template.constraints,
                },
                id: "proposalId".to_string(),
                issuer: Default::default(),
                state: State::Initial,
                timestamp: Utc::now(),
            }
        }
    }

    /// Negotiator accepts demand if all of the required fields exist
    #[test]
    fn test_required_fields_exist() {
        let config = config();
        let mut negotiator = DemandValidation::new(&config);

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let demand = properties_to_proposal(json!({
            "golem.com.freebies": "mug",
            "golem.com.payment.address": "0x123",
        }));

        let expected_result = NegotiationResult::Ready {
            offer: offer_proposal.clone(),
        };
        assert_eq!(
            negotiator.negotiate_step(&demand, offer_proposal).unwrap(),
            expected_result
        );
    }

    /// Negotiator rejects demand if some of the required fields are missing
    #[test]
    fn test_required_fields_missing() {
        let config = config();
        let mut negotiator = DemandValidation::new(&config);

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let demand = properties_to_proposal(json!({
            "golem.com.freebies": "mug",
        }));

        let expected_result = NegotiationResult::Reject {
            message: "Missing fields: /golem/com/payment/address".to_string(),
            is_final: false,
        };
        assert_eq!(
            negotiator.negotiate_step(&demand, offer_proposal).unwrap(),
            expected_result
        );
    }
}
