use chrono::Duration;

use ya_agreement_utils::{Error, OfferDefinition};

use crate::display::EnableDisplay;
use crate::market::negotiator::factory::PaymentTimeoutConfig;
use crate::market::negotiator::{
    AgreementResult, NegotiationResult, NegotiatorComponent, ProposalView,
};

const PAYMENT_TIMEOUT_PROPERTY_FLAT: &'static str = "golem.com.scheme.payu.payment-timeout-sec?";
pub const PAYMENT_TIMEOUT_PROPERTY: &'static str = "/golem/com/scheme/payu/payment-timeout-sec?";

/// PaymentTimeout negotiator
pub struct PaymentTimeout {
    min_timeout: Duration,
    max_timeout: Duration,
    timeout: Duration,
}

impl PaymentTimeout {
    pub fn new(config: &PaymentTimeoutConfig) -> anyhow::Result<Self> {
        let min_timeout = Duration::from_std(config.min_payment_timeout)?;
        let max_timeout = Duration::from_std(config.max_payment_timeout)?;
        let timeout = Duration::from_std(config.payment_timeout)?;

        if min_timeout > max_timeout {
            anyhow::bail!(
                "Minimum payment timeout {} is greater than the maximum of {}",
                min_timeout.display(),
                max_timeout.display()
            );
        }

        Ok(Self {
            min_timeout,
            max_timeout,
            timeout,
        })
    }
}

impl NegotiatorComponent for PaymentTimeout {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        mut offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        let offer_timeout = read_duration(PAYMENT_TIMEOUT_PROPERTY, &offer)?;
        let demand_timeout = read_duration(PAYMENT_TIMEOUT_PROPERTY, demand)?;

        match demand_timeout {
            Some(timeout) => {
                let offer_timeout = offer_timeout.ok_or_else(|| {
                    anyhow::anyhow!("DebitNote payment timeout not found in the Offer")
                })?;

                if timeout < self.min_timeout || timeout > self.max_timeout {
                    return Ok(NegotiationResult::Reject {
                        message: format!(
                            "Demand DebitNote payment timeout {} not in acceptable range of [{}; {}]",
                            timeout.display(),
                            self.min_timeout.display(),
                            self.max_timeout.display(),
                        ),
                        is_final: true,
                    });
                } else if offer_timeout != timeout {
                    let property = offer.pointer_mut(PAYMENT_TIMEOUT_PROPERTY).unwrap();
                    *property = serde_json::Value::Number(timeout.num_seconds().into());
                    return Ok(NegotiationResult::Negotiating { offer });
                }
            }
            None => {
                if offer_timeout.is_some() {
                    let _ = offer.remove_property(PAYMENT_TIMEOUT_PROPERTY);
                    return Ok(NegotiationResult::Negotiating { offer });
                }
            }
        }

        Ok(NegotiationResult::Ready { offer })
    }

    fn fill_template(
        &mut self,
        mut offer_template: OfferDefinition,
    ) -> anyhow::Result<OfferDefinition> {
        offer_template.offer.set_property(
            PAYMENT_TIMEOUT_PROPERTY_FLAT,
            serde_json::Value::Number(self.timeout.num_seconds().into()),
        );
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

fn read_duration(pointer: &str, proposal: &ProposalView) -> anyhow::Result<Option<Duration>> {
    match proposal.pointer_typed::<u32>(pointer) {
        Ok(val) => Ok(Some(Duration::seconds(val as i64))),
        Err(Error::NoKey { .. }) => Ok(None),
        Err(err) => Err(err.into()),
    }
}
