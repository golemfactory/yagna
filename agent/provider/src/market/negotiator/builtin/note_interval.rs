use chrono::Duration;

use ya_agreement_utils::{Error, OfferDefinition};

use crate::display::EnableDisplay;
use crate::market::negotiator::factory::DebitNoteIntervalConfig;
use crate::market::negotiator::{
    AgreementResult, NegotiationResult, NegotiatorComponent, ProposalView,
};

pub const DEFAULT_DEBIT_NOTE_INTERVAL_SEC: u32 = 120;
pub const DEBIT_NOTE_INTERVAL_PROPERTY: &'static str =
    "/golem/com/scheme/payu/debit-note/interval-sec?";
const DEBIT_NOTE_INTERVAL_PROPERTY_FLAT: &'static str =
    "golem.com.scheme.payu.debit-note.interval-sec?";

/// DebitNoteInterval negotiator
pub struct DebitNoteInterval {
    min_interval: Duration,
    max_interval: Duration,
    interval: Duration,
}

impl DebitNoteInterval {
    pub fn new(config: &DebitNoteIntervalConfig) -> anyhow::Result<Self> {
        let min_interval = Duration::from_std(config.min_debit_note_interval)?;
        let max_interval = Duration::from_std(config.max_debit_note_interval)?;

        if min_interval > max_interval {
            anyhow::bail!(
                "Minimum debit note interval {} is greater than the maximum of {}",
                min_interval.display(),
                max_interval.display()
            );
        }

        Ok(Self {
            min_interval,
            max_interval,
            interval: Duration::from_std(config.debit_note_interval)?,
        })
    }
}

impl NegotiatorComponent for DebitNoteInterval {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        mut offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        let offer_interval = read_duration(DEBIT_NOTE_INTERVAL_PROPERTY, &offer)?;
        let demand_interval = read_duration(DEBIT_NOTE_INTERVAL_PROPERTY, demand)?;

        if let Some(interval) = demand_interval {
            let offer_interval = offer_interval
                .ok_or_else(|| anyhow::anyhow!("DebitNote interval not found in the Offer"))?;

            if interval < self.min_interval || interval > self.max_interval {
                return Ok(NegotiationResult::Reject {
                    message: format!(
                        "Demand DebitNote interval {} not in acceptable range of [{}; {}]",
                        interval.display(),
                        self.min_interval.display(),
                        self.max_interval.display(),
                    ),
                    is_final: true,
                });
            } else if offer_interval != interval {
                let property = offer.pointer_mut(DEBIT_NOTE_INTERVAL_PROPERTY).unwrap();
                *property = serde_json::Value::Number(interval.num_seconds().into());
                return Ok(NegotiationResult::Negotiating { offer });
            }
        }

        Ok(NegotiationResult::Ready { offer })
    }

    fn fill_template(
        &mut self,
        mut offer_template: OfferDefinition,
    ) -> anyhow::Result<OfferDefinition> {
        offer_template.offer.set_property(
            DEBIT_NOTE_INTERVAL_PROPERTY_FLAT,
            serde_json::Value::Number(self.interval.num_seconds().into()),
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
