use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use structopt::StructOpt;

use ya_negotiators::component::{
    NegotiationResult, NegotiatorComponentMut, NegotiatorFactory, NegotiatorMut, ProposalView,
    RejectReason, Score,
};

/// Negotiator that verifies that all required fields are present in proposal.
pub struct DemandValidation {
    required_fields: Vec<String>,
}

/// Configuration for Demand Validation Negotiator.
#[derive(StructOpt, Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[structopt(long, default_value = "/golem/com/payment/chosen-platform")]
    pub required_fields: Vec<String>,
}

impl DemandValidation {
    pub fn new(config: &Config) -> DemandValidation {
        let required_fields = config.required_fields.clone();
        Self { required_fields }
    }
}

impl NegotiatorFactory<DemandValidation> for DemandValidation {
    type Type = NegotiatorMut;

    fn new(
        _name: &str,
        config: serde_yaml::Value,
        _agent_env: serde_yaml::Value,
        _workdir: PathBuf,
    ) -> anyhow::Result<DemandValidation> {
        let config: Config = serde_yaml::from_value(config)?;
        Ok(Self {
            required_fields: config.required_fields.clone(),
        })
    }
}

impl NegotiatorComponentMut for DemandValidation {
    fn negotiate_step(
        &mut self,
        their: &ProposalView,
        ours: ProposalView,
        score: Score,
    ) -> anyhow::Result<NegotiationResult> {
        let missing_fields = self
            .required_fields
            .iter()
            .filter(|x| their.pointer(x).is_none())
            .cloned()
            .collect::<Vec<String>>();
        if missing_fields.is_empty() {
            Ok(NegotiationResult::Ready {
                proposal: ours,
                score,
            })
        } else {
            log::info!(
                "'DemandValidation' negotiator: Reject proposal [{}] due to missing fields: {}",
                their.id,
                missing_fields.join(",")
            );
            Ok(NegotiationResult::Reject {
                reason: RejectReason::new(format!("Missing fields: {}", missing_fields.join(","))),
                is_final: false,
            })
        }
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

    fn config() -> Config {
        Config {
            required_fields: vec![
                "/golem/com/freebies".to_string(),
                "/golem/com/payment/address".to_string(),
            ],
        }
    }

    fn properties_to_proposal(properties: serde_json::Value) -> ProposalView {
        ProposalView {
            content: OfferTemplate {
                properties: expand(properties),
                constraints: "()".to_string(),
            },
            id: "proposalId".to_string(),
            issuer: Default::default(),
            state: State::Initial,
            timestamp: Utc::now(),
        }
    }

    /// Negotiator accepts demand if all of the required fields exist
    #[test]
    fn test_required_fields_exist() {
        let config = config();
        let mut negotiator = DemandValidation::new(&config);

        let offer = properties_to_proposal(json!({}));
        let demand = properties_to_proposal(json!({
            "golem.com.freebies": "mug",
            "golem.com.payment.address": "0x123",
        }));
        let score = Score::default();

        let expected_result = NegotiationResult::Ready {
            proposal: offer.clone(),
            score: score.clone(),
        };
        assert_eq!(
            negotiator.negotiate_step(&demand, offer, score).unwrap(),
            expected_result
        );
    }

    /// Negotiator rejects demand if some of the required fields are missing
    #[test]
    fn test_required_fields_missing() {
        let config = config();
        let mut negotiator = DemandValidation::new(&config);

        let offer = properties_to_proposal(json!({}));
        let demand = properties_to_proposal(json!({
            "golem.com.freebies": "mug",
        }));
        let score = Score::default();

        let expected_result = NegotiationResult::Reject {
            reason: RejectReason::new("Missing fields: /golem/com/payment/address"),
            is_final: false,
        };
        assert_eq!(
            negotiator.negotiate_step(&demand, offer, score).unwrap(),
            expected_result
        );
    }
}
