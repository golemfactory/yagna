use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};

use ya_agreement_utils::{Error, OfferDefinition};
use ya_client_model::market::Reason;

use crate::display::EnableDisplay;
use crate::market::negotiator::factory::AgreementExpirationNegotiatorConfig;
use crate::market::negotiator::{
    AgreementResult, NegotiationResult, NegotiatorComponent, ProposalView,
};

/// Negotiator that can reject Requestors, that request too long Agreement
/// expiration time. Expiration limit can be different in case, when Requestor
/// promises to accept DebitNotes in deadline specified in Agreement.
pub struct LimitExpiration {
    min_expiration: Duration,
    max_expiration: Duration,

    /// DebitNote acceptance timeout. Base point for negotiations.
    payment_deadline: Duration,
    /// If Requestor doesn't promise to accept DebitNotes, this alternative max_expiration will be used.
    max_expiration_without_deadline: Duration,

    /// Minimal DebitNote acceptance timeout.
    min_deadline: i64,
}

pub static DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY: &'static str =
    "/golem/com/payment/debit-notes/accept-timeout?";
pub static AGREEMENT_EXPIRATION_PROPERTY: &'static str = "/golem/srv/comp/expiration";

// TODO: We should unify properties access in agreement-utils, because it is annoying to use both forms.
pub static DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT: &'static str =
    "golem.com.payment.debit-notes.accept-timeout?";

// Note: Tests are using this.
#[allow(dead_code)]
pub static AGREEMENT_EXPIRATION_PROPERTY_FLAT: &'static str = "golem.srv.comp.expiration";

impl LimitExpiration {
    pub fn new(config: &AgreementExpirationNegotiatorConfig) -> anyhow::Result<LimitExpiration> {
        let component = LimitExpiration {
            min_expiration: chrono::Duration::from_std(config.min_agreement_expiration)?,
            max_expiration: chrono::Duration::from_std(config.max_agreement_expiration)?,
            payment_deadline: chrono::Duration::from_std(config.debit_note_acceptance_deadline)?,
            max_expiration_without_deadline: chrono::Duration::from_std(
                config.max_agreement_expiration_without_deadline,
            )?,
            min_deadline: 5,
        };

        if component.payment_deadline.num_seconds() < component.min_deadline {
            return Err(anyhow!(
                "To low DebitNotes deadline: {}",
                component.payment_deadline.display()
            ));
        }

        Ok(component)
    }
}

fn proposal_expiration_from(proposal: &ProposalView) -> Result<DateTime<Utc>> {
    let value = proposal
        .pointer(AGREEMENT_EXPIRATION_PROPERTY)
        .ok_or_else(|| anyhow::anyhow!("Missing expiration key in Proposal"))?
        .clone();
    let timestamp: i64 = serde_json::from_value(value)?;
    Ok(Utc.timestamp_millis(timestamp))
}

fn debit_deadline_from(proposal: &ProposalView) -> Result<Option<Duration>> {
    match proposal.pointer_typed::<i64>(DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY) {
        // Requestor is able to accept DebitNotes, because he set this property.
        Ok(deadline) => Ok(Some(Duration::seconds(deadline))),
        // If he didn't set this property, he is unable to accept DebitNotes.
        Err(Error::NoKey { .. }) => Ok(None),
        // Property has invalid type. We shouldn't continue negotiations, since
        // Requestor probably doesn't understand specification.
        Err(e) => Err(e.into()),
    }
}

impl NegotiatorComponent for LimitExpiration {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        mut offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        let req_deadline = debit_deadline_from(demand)?;
        let our_deadline = debit_deadline_from(&offer)?;
        let req_expiration = proposal_expiration_from(&demand)?;

        // Let's check if Requestor is able to accept DebitNotes.
        let max_expiration_delta = match &req_deadline {
            Some(_) => self.max_expiration,
            None => self.max_expiration_without_deadline,
        };

        let now = Utc::now();
        let max_expiration = now + max_expiration_delta;
        let min_expiration = now + self.min_expiration;

        if req_expiration > max_expiration || req_expiration < min_expiration {
            log::info!(
                "Negotiator: Reject proposal [{}] due to expiration limits.",
                demand.agreement_id
            );
            return Ok(NegotiationResult::Reject {
                reason: Some(Reason::new(format!(
                    "Proposal expires at: {} which is less than {} or more than {} from now",
                    req_expiration,
                    self.min_expiration.display(),
                    max_expiration_delta.display()
                ))),
            });
        };

        // Maybe we negotiated different deadline in previous negotiation iteration?
        Ok(match (req_deadline, our_deadline) {
            // Both Provider and Requestor support DebitNotes acceptance. We must
            // negotiate until we will agree to the same value.
            (Some(req_deadline), Some(our_deadline)) => {
                if req_deadline > our_deadline {
                    NegotiationResult::Reject {
                        reason: Some(Reason::new(format!(
                            "DebitNote acceptance deadline should be less than {}.",
                            self.payment_deadline.display()
                        ))),
                    }
                } else if req_deadline == our_deadline {
                    // We agree with Requestor to the same deadline.
                    NegotiationResult::Ready { offer }
                } else {
                    // Below certain timeout it is impossible for Requestor to accept DebitNotes.
                    if req_deadline.num_seconds() < self.min_deadline {
                        return Ok(NegotiationResult::Reject {
                            reason: Some(Reason::new(format!(
                                "To low DebitNotes timeout: {}",
                                req_deadline.display()
                            ))),
                        });
                    }

                    // Requestor proposed better deadline, than we required.
                    // We are expected to set property to the same value if we agree.
                    let deadline_prop = offer
                        .pointer_mut(DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY)
                        .unwrap();
                    *deadline_prop = serde_json::Value::Number(req_deadline.num_seconds().into());

                    // Since we changed our proposal, we can't return `Ready`.
                    NegotiationResult::Negotiating { offer }
                }
            }
            // Requestor doesn't support DebitNotes acceptance, so we should
            // remove our property from Proposal to match with his.
            (None, Some(_)) => {
                offer.remove_property(DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY)?;
                NegotiationResult::Negotiating { offer }
            }
            // We agree with Requestor, that he won't accept DebitNotes.
            (None, None) => NegotiationResult::Ready { offer },
            _ => return Err(anyhow!("Shouldn't be in this state.")),
        })
    }

    fn fill_template(&mut self, mut template: OfferDefinition) -> anyhow::Result<OfferDefinition> {
        template.offer.set_property(
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT,
            serde_json::Value::Number(self.payment_deadline.num_seconds().into()),
        );
        Ok(template)
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
mod test_expiration_negotiator {
    use super::*;
    use ya_agreement_utils::agreement::expand;
    use ya_agreement_utils::{InfNodeInfo, NodeInfo, OfferTemplate, ServiceInfo};

    fn expiration_config() -> AgreementExpirationNegotiatorConfig {
        AgreementExpirationNegotiatorConfig {
            min_agreement_expiration: std::time::Duration::from_secs(5 * 60),
            max_agreement_expiration: std::time::Duration::from_secs(30 * 60),
            max_agreement_expiration_without_deadline: std::time::Duration::from_secs(10 * 60),
            debit_note_acceptance_deadline: std::time::Duration::from_secs(120),
        }
    }

    fn properties_to_proposal(value: serde_json::Value) -> ProposalView {
        ProposalView {
            agreement_id: "2332850934yer".to_string(),
            json: expand(value),
        }
    }

    fn example_offer() -> OfferDefinition {
        OfferDefinition {
            node_info: NodeInfo::with_name("nanana"),
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
            ProposalView {
                agreement_id: "sagdshgdfgd".to_string(),
                json: expand(self.into_json()),
            }
        }
    }

    /// Negotiator accepts lower deadline (which is better for him) and
    /// adjusts his property to match Requestor's.
    /// Provider should use `max_agreement_expiration` value, when checking expiration.
    #[test]
    fn test_lower_deadline() {
        let config = expiration_config();
        let mut negotiator = LimitExpiration::new(&config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(15)).timestamp_millis(),
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT: 50,
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal)
            .unwrap()
        {
            // Negotiator is expected to take better proposal and change adjust property.
            NegotiationResult::Negotiating { offer } => {
                assert_eq!(
                    debit_deadline_from(&offer).unwrap().unwrap(),
                    Duration::seconds(50)
                )
            }
            result => panic!("Expected NegotiationResult::Negotiating. Got: {:?}", result),
        }
    }

    /// Negotiator rejects Proposals with deadline greater than he expects.
    #[test]
    fn test_greater_deadline() {
        let config = expiration_config();
        let mut negotiator = LimitExpiration::new(&config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(7)).timestamp_millis(),
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT: 130,
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal)
            .unwrap()
        {
            NegotiationResult::Reject { reason } => {
                assert!(reason
                    .unwrap()
                    .message
                    .contains("DebitNote acceptance deadline should be less than"))
            }
            result => panic!("Expected NegotiationResult::Reject. Got: {:?}", result),
        }
    }

    /// Negotiator accepts the same deadline property. Negotiation is ready
    /// to create Agreement from this Proposal.
    #[test]
    fn test_equal_deadline() {
        let config = expiration_config();
        let mut negotiator = LimitExpiration::new(&config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(7)).timestamp_millis(),
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT: 120,
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal)
            .unwrap()
        {
            NegotiationResult::Ready { offer } => {
                assert_eq!(
                    debit_deadline_from(&offer).unwrap().unwrap(),
                    Duration::seconds(120)
                )
            }
            result => panic!("Expected NegotiationResult::Ready. Got: {:?}", result),
        }
    }

    /// Requestor doesn't declare that he is able to accept DebitNotes, but demands
    /// to high expirations time.
    /// Provider should use `max_agreement_expiration_without_deadline` config
    /// value for expiration in this case and reject Proposal.
    #[test]
    fn test_requestor_doesnt_accept_debit_notes_to_high_expiration() {
        let config = expiration_config();
        let mut negotiator = LimitExpiration::new(&config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(15)).timestamp_millis(),
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal)
            .unwrap()
        {
            NegotiationResult::Reject { reason } => {
                assert!(reason.unwrap().message.contains("Proposal expires at"))
            }
            result => panic!("Expected NegotiationResult::Reject. Got: {:?}", result),
        }
    }

    /// Requestor isn't able to accept DebitNotes, but he sets expirations below
    /// Provider's limit.
    /// Property related to DebitNotes deadline should be removed. Provider should
    /// return Negotiating state, because he had to remove property.
    #[test]
    fn test_requestor_doesnt_accept_debit_notes_expiration_ok() {
        let config = expiration_config();
        let mut negotiator = LimitExpiration::new(&config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(7)).timestamp_millis(),
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal)
            .unwrap()
        {
            NegotiationResult::Negotiating { offer } => {
                assert!(debit_deadline_from(&offer).unwrap().is_none())
            }
            result => panic!("Expected NegotiationResult::Negotiating. Got: {:?}", result),
        }
    }
}
