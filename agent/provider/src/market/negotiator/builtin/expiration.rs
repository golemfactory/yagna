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

    /// If deadline is None, we allow, that Requestor doesn't accept DebitNotes.
    payment_deadline: Duration,
    /// If Requestor doesn't promise to accept DebitNotes, this alternative max_expiration will be used.
    max_expiration_without_deadline: Duration,
}

impl LimitExpiration {
    pub fn new(config: &AgreementExpirationNegotiatorConfig) -> anyhow::Result<LimitExpiration> {
        Ok(LimitExpiration {
            min_expiration: chrono::Duration::from_std(config.min_agreement_expiration)?,
            max_expiration: chrono::Duration::from_std(config.max_agreement_expiration)?,
            payment_deadline: chrono::Duration::from_std(config.debit_note_acceptance_deadline)?,
            max_expiration_without_deadline: chrono::Duration::from_std(
                config.max_agreement_expiration_without_deadline,
            )?,
        })
    }
}

fn proposal_expiration_from(proposal: &ProposalView) -> Result<DateTime<Utc>> {
    let expiration_key_str = "/golem/srv/comp/expiration";
    let value = proposal
        .pointer(expiration_key_str)
        .ok_or_else(|| anyhow::anyhow!("Missing expiration key in Proposal"))?
        .clone();
    let timestamp: i64 = serde_json::from_value(value)?;
    Ok(Utc.timestamp_millis(timestamp))
}

fn debit_deadline_from(proposal: &ProposalView) -> Result<Option<Duration>> {
    match proposal.pointer_typed::<i64>("/golem/com/payment/debit-notes/acceptance-deadline") {
        // Requestor can accept DebitNotes, because he set this property.
        Ok(deadline) => Ok(Some(Duration::seconds(deadline))),
        // If he didn't set this property, he can't accept DebitNotes.
        Err(Error::NoKey { .. }) => Ok(None),
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
                demand.id
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
                if req_deadline < our_deadline {
                    NegotiationResult::Reject {
                        reason: Some(Reason::new(format!(
                            "DebitNote acceptance deadline should be greater than {}.",
                            self.payment_deadline
                        ))),
                    }
                } else if req_deadline == our_deadline {
                    // We agree with Requestor to the same deadline.
                    NegotiationResult::Ready { offer }
                } else {
                    // Requestor proposed better deadline, than we required.
                    // We are expected to set property to the same value if we agree.
                    let deadline_prop = offer
                        .pointer_mut("/golem/com/payment/debit-notes/acceptance-deadline")
                        .unwrap();
                    *deadline_prop = serde_json::Value::Number(req_deadline.num_seconds().into());

                    // Since we changed our proposal, we can't return `Ready`.
                    NegotiationResult::Negotiating { offer }
                }
            }
            // Requestor doesn't support DebitNotes acceptance, so we should
            // remove our property from Proposal to match with his.
            (None, Some(_)) => {
                offer.remove_property("/golem/com/payment/debit-notes/acceptance-deadline")?;
                NegotiationResult::Negotiating { offer }
            }
            // We agree with Requestor, that he won't accept DebitNotes.
            (None, None) => NegotiationResult::Ready { offer },
            _ => return Err(anyhow!("Shouldn't be in this state.")),
        })
    }

    fn fill_template(&mut self, mut template: OfferDefinition) -> anyhow::Result<OfferDefinition> {
        template.offer.set_property(
            "golem.com.payment.debit-notes.acceptance-deadline",
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
