use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use humantime;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

use ya_agreement_utils::{Error, OfferTemplate, ProposalView};
use ya_negotiators::component::{RejectReason, Score};
use ya_negotiators::factory::{LoadMode, NegotiatorConfig};
use ya_negotiators::{NegotiationResult, NegotiatorComponent};

use crate::display::EnableDisplay;

/// Negotiator that can reject Requestors, that request too long Agreement
/// expiration time. Expiration limit can be different in case, when Requestor
/// promises to accept DebitNotes in deadline specified in Agreement.
pub struct LimitExpiration {
    min_expiration: Duration,
    max_expiration: Duration,

    /// DebitNote acceptance timeout. Base point for negotiations.
    accept_timeout: Duration,
    /// If Requestor doesn't promise to accept DebitNotes, this alternative max_expiration will be used.
    max_expiration_without_deadline: Duration,

    /// Minimal DebitNote acceptance timeout.
    min_deadline: i64,
}

pub static DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY: &str =
    "/golem/com/payment/debit-notes/accept-timeout?";
pub static AGREEMENT_EXPIRATION_PROPERTY: &str = "/golem/srv/comp/expiration";

// TODO: We should unify properties access in agreement-utils, because it is annoying to use both forms.
pub static DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT: &str =
    "golem.com.payment.debit-notes.accept-timeout?";

// Note: Tests are using this.
#[allow(dead_code)]
pub static AGREEMENT_EXPIRATION_PROPERTY_FLAT: &str = "golem.srv.comp.expiration";

/// Configuration for LimitAgreements Negotiator.
#[derive(StructOpt, Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "5min")]
    pub min_agreement_expiration: std::time::Duration,
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "10h")]
    pub max_agreement_expiration: std::time::Duration,
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "30min")]
    pub max_agreement_expiration_without_deadline: std::time::Duration,
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "4min")]
    pub debit_note_acceptance_deadline: std::time::Duration,
}

impl LimitExpiration {
    pub fn new(config: serde_yaml::Value) -> anyhow::Result<LimitExpiration> {
        let config: Config = serde_yaml::from_value(config)?;
        let component = LimitExpiration {
            min_expiration: chrono::Duration::from_std(config.min_agreement_expiration)?,
            max_expiration: chrono::Duration::from_std(config.max_agreement_expiration)?,
            accept_timeout: chrono::Duration::from_std(config.debit_note_acceptance_deadline)?,
            max_expiration_without_deadline: chrono::Duration::from_std(
                config.max_agreement_expiration_without_deadline,
            )?,
            min_deadline: 5,
        };

        if component.accept_timeout.num_seconds() < component.min_deadline {
            return Err(anyhow!(
                "To low DebitNotes deadline: {}",
                component.accept_timeout.display()
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

    match Utc.timestamp_millis_opt(timestamp) {
        chrono::LocalResult::Single(t) => Ok(t),
        _ => Err(anyhow!("Cannot make DateTime from timestamp {timestamp}")),
    }
}

fn debit_deadline_from(proposal: &ProposalView) -> Result<Option<Duration>> {
    match proposal.pointer_typed::<u32>(DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY) {
        // Requestor is able to accept DebitNotes, because he set this property.
        Ok(deadline) => Ok(Some(Duration::seconds(deadline as i64))),
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
        their: &ProposalView,
        mut ours: ProposalView,
        score: Score,
    ) -> Result<NegotiationResult> {
        let req_deadline = debit_deadline_from(their)?;
        let our_deadline = debit_deadline_from(&ours)?;
        let req_expiration = proposal_expiration_from(&their)?;

        // Let's check if Requestor is able to accept DebitNotes.
        let max_expiration_delta = match &req_deadline {
            Some(_) => self.max_expiration,
            None => self.max_expiration_without_deadline,
        };

        let now = Utc::now();
        let max_expiration = now + max_expiration_delta;
        let min_expiration = now + self.min_expiration;

        let too_late = req_expiration < min_expiration;
        let too_soon = req_expiration > max_expiration;
        if too_soon || too_late {
            log::info!(
                "Negotiator: Reject proposal [{}] due to expiration limits.",
                their.id
            );

            return Ok(NegotiationResult::Reject {
                reason: RejectReason::new(format!(
                    "Proposal expires at: {} which is less than {} or more than {} from now",
                    req_expiration,
                    self.min_expiration.display(),
                    max_expiration_delta.display()
                )),
                is_final: too_late, // when it's too soon we could try later
            });
        };

        // Maybe we negotiated different deadline in previous negotiation iteration?
        Ok(match (req_deadline, our_deadline) {
            // Both Provider and Requestor support DebitNotes acceptance. We must
            // negotiate until we will agree to the same value.
            (Some(req_deadline), Some(our_deadline)) => {
                match req_deadline.cmp(&our_deadline) {
                    std::cmp::Ordering::Greater => NegotiationResult::Reject {
                        reason: RejectReason::new(format!(
                            "DebitNote acceptance deadline should be less than {}.",
                            self.accept_timeout.display()
                        )),
                        is_final: true,
                    },
                    std::cmp::Ordering::Equal => {
                        // We agree with Requestor to the same deadline.
                        NegotiationResult::Ready {
                            proposal: ours,
                            score,
                        }
                    }
                    std::cmp::Ordering::Less => {
                        // Below certain timeout it is impossible for Requestor to accept DebitNotes.
                        if req_deadline.num_seconds() < self.min_deadline {
                            return Ok(NegotiationResult::Reject {
                                reason: RejectReason::new(format!(
                                    "To low DebitNotes timeout: {}",
                                    req_deadline.display()
                                )),
                                is_final: true,
                            });
                        }

                        // Requestor proposed better deadline, than we required.
                        // We are expected to set property to the same value if we agree.
                        let deadline_prop = ours
                            .pointer_mut(DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY)
                            .unwrap();
                        *deadline_prop =
                            serde_json::Value::Number(req_deadline.num_seconds().into());

                        // Since we changed our proposal, we can't return `Ready`.
                        NegotiationResult::Negotiating {
                            proposal: ours,
                            score,
                        }
                    }
                }
            }
            // Requestor doesn't support DebitNotes acceptance, so we should
            // remove our property from Proposal to match with his.
            (None, Some(_)) => {
                ours.remove_property(DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY)?;
                NegotiationResult::Negotiating {
                    proposal: ours,
                    score,
                }
            }
            // We agree with Requestor, that he won't accept DebitNotes.
            (None, None) => NegotiationResult::Ready {
                proposal: ours,
                score,
            },
            _ => return Err(anyhow!("Shouldn't be in this state.")),
        })
    }

    fn fill_template(&mut self, mut template: OfferTemplate) -> Result<OfferTemplate> {
        template.set_property(
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT,
            serde_json::Value::Number(self.accept_timeout.num_seconds().into()),
        );
        Ok(template)
    }
}

impl Config {
    pub fn from_env() -> Result<NegotiatorConfig> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        let config = Config::from_iter_safe(&[""])?;
        Ok(NegotiatorConfig {
            name: "LimitExpiration".to_string(),
            load_mode: LoadMode::StaticLib {
                library: "ya-provider".to_string(),
            },
            params: serde_yaml::to_value(&config)?,
        })
    }
}

#[cfg(test)]
mod test_expiration_negotiator {
    use super::*;

    use ya_agreement_utils::agreement::expand;
    use ya_agreement_utils::{InfNodeInfo, NodeInfo, OfferDefinition, OfferTemplate, ServiceInfo};
    use ya_client_model::market::proposal::State;

    fn expiration_config() -> serde_yaml::Value {
        serde_yaml::to_value(&Config {
            min_agreement_expiration: std::time::Duration::from_secs(5 * 60),
            max_agreement_expiration: std::time::Duration::from_secs(30 * 60),
            max_agreement_expiration_without_deadline: std::time::Duration::from_secs(10 * 60),
            debit_note_acceptance_deadline: std::time::Duration::from_secs(120),
        })
        .unwrap()
    }

    fn properties_to_proposal(value: serde_json::Value) -> ProposalView {
        ProposalView {
            content: OfferTemplate {
                properties: expand(value),
                constraints: "()".to_string(),
            },
            id: "2332850934yer".to_string(),
            issuer: Default::default(),
            state: State::Initial,
            timestamp: Utc::now(),
        }
    }

    fn example_offer() -> OfferTemplate {
        OfferDefinition {
            node_info: NodeInfo::with_name("nanana"),
            srv_info: ServiceInfo::new(InfNodeInfo::default(), serde_json::Value::Null),
            com_info: Default::default(),
            offer: OfferTemplate::default(),
        }
        .into_template()
    }

    trait ToProposal {
        fn to_proposal(self) -> ProposalView;
    }

    impl ToProposal for OfferTemplate {
        fn to_proposal(self) -> ProposalView {
            let template = self.into_template();
            ProposalView {
                content: OfferTemplate {
                    properties: expand(template.properties),
                    constraints: template.constraints,
                },
                id: "sagdshgdfgd".to_string(),
                issuer: Default::default(),
                state: State::Initial,
                timestamp: Utc::now(),
            }
        }
    }

    /// Negotiator accepts lower deadline (which is better for him) and
    /// adjusts his property to match Requestor's.
    /// Provider should use `max_agreement_expiration` value, when checking expiration.
    #[test]
    fn test_lower_deadline() {
        let config = expiration_config();
        let mut negotiator = LimitExpiration::new(config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(15)).timestamp_millis(),
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT: 50,
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal, Score::default())
            .unwrap()
        {
            // Negotiator is expected to take better proposal and change adjust property.
            NegotiationResult::Negotiating {
                proposal: offer, ..
            } => {
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
        let mut negotiator = LimitExpiration::new(config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(7)).timestamp_millis(),
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT: 130,
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal, Score::default())
            .unwrap()
        {
            NegotiationResult::Reject { reason, is_final } => {
                assert!(reason
                    .message
                    .contains("DebitNote acceptance deadline should be less than"));
                assert!(is_final)
            }
            result => panic!("Expected NegotiationResult::Reject. Got: {:?}", result),
        }
    }

    /// Negotiator accepts the same deadline property. Negotiation is ready
    /// to create Agreement from this Proposal.
    #[test]
    fn test_equal_deadline() {
        let config = expiration_config();
        let mut negotiator = LimitExpiration::new(config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(7)).timestamp_millis(),
            DEBIT_NOTE_ACCEPT_TIMEOUT_PROPERTY_FLAT: 120,
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal, Score::default())
            .unwrap()
        {
            NegotiationResult::Ready {
                proposal: offer, ..
            } => {
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
        let mut negotiator = LimitExpiration::new(config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(15)).timestamp_millis(),
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal, Score::default())
            .unwrap()
        {
            NegotiationResult::Reject { reason, is_final } => {
                assert!(reason.message.contains("Proposal expires at"));
                assert!(!is_final)
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
        let mut negotiator = LimitExpiration::new(config).unwrap();

        let offer_proposal = negotiator
            .fill_template(example_offer())
            .unwrap()
            .to_proposal();

        let proposal = properties_to_proposal(serde_json::json!({
            AGREEMENT_EXPIRATION_PROPERTY_FLAT: (Utc::now() + Duration::minutes(7)).timestamp_millis(),
        }));

        match negotiator
            .negotiate_step(&proposal, offer_proposal, Score::default())
            .unwrap()
        {
            NegotiationResult::Negotiating {
                proposal: offer, ..
            } => {
                assert!(debit_deadline_from(&offer).unwrap().is_none())
            }
            result => panic!("Expected NegotiationResult::Negotiating. Got: {:?}", result),
        }
    }
}
