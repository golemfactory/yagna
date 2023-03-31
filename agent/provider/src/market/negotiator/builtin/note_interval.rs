use chrono::Duration;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use structopt::StructOpt;

use ya_negotiators::agreement::{Error, OfferTemplate, ProposalView};
use ya_negotiators::component::{
    NegotiationResult, NegotiatorComponentMut, NegotiatorFactory, NegotiatorMut, RejectReason,
    Score,
};
use ya_negotiators::factory::{LoadMode, NegotiatorConfig};

use crate::display::EnableDisplay;

pub const DEFAULT_DEBIT_NOTE_INTERVAL_SEC: u32 = 120;
pub const DEBIT_NOTE_INTERVAL_PROPERTY: &str = "/golem/com/scheme/payu/debit-note/interval-sec?";
const DEBIT_NOTE_INTERVAL_PROPERTY_FLAT: &str = "golem.com.scheme.payu.debit-note.interval-sec?";

/// DebitNoteInterval negotiator
pub struct DebitNoteInterval {
    min_interval: Duration,
    max_interval: Duration,
    interval: Duration,
}

/// Configuration for DebitNoteInterval negotiator
#[derive(StructOpt, Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "1s")]
    pub min_debit_note_interval: std::time::Duration,
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "24h")]
    pub max_debit_note_interval: std::time::Duration,
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "2min")]
    pub debit_note_interval: std::time::Duration,
}

impl NegotiatorFactory<DebitNoteInterval> for DebitNoteInterval {
    type Type = NegotiatorMut;

    fn new(
        _name: &str,
        config: serde_yaml::Value,
        _agent_env: serde_yaml::Value,
        _workdir: PathBuf,
    ) -> anyhow::Result<DebitNoteInterval> {
        let config: Config = serde_yaml::from_value(config)?;

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

impl NegotiatorComponentMut for DebitNoteInterval {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        mut offer: ProposalView,
        score: Score,
    ) -> anyhow::Result<NegotiationResult> {
        let offer_interval = read_duration(DEBIT_NOTE_INTERVAL_PROPERTY, &offer)?;
        let demand_interval = read_duration(DEBIT_NOTE_INTERVAL_PROPERTY, demand)?;

        match demand_interval {
            Some(interval) => {
                let offer_interval = offer_interval
                    .ok_or_else(|| anyhow::anyhow!("DebitNote interval not found in the Offer"))?;

                if interval < self.min_interval || interval > self.max_interval {
                    return Ok(NegotiationResult::Reject {
                        reason: RejectReason::new(format!(
                            "Demand DebitNote interval {} not in acceptable range of [{}; {}]",
                            interval.display(),
                            self.min_interval.display(),
                            self.max_interval.display(),
                        )),
                        is_final: true,
                    });
                } else if offer_interval != interval {
                    let property = offer.pointer_mut(DEBIT_NOTE_INTERVAL_PROPERTY).unwrap();
                    *property = serde_json::Value::Number(interval.num_seconds().into());
                    return Ok(NegotiationResult::Negotiating {
                        proposal: offer,
                        score,
                    });
                }
            }
            None => {
                if offer_interval.is_some() {
                    let _ = offer.remove_property(DEBIT_NOTE_INTERVAL_PROPERTY);
                    return Ok(NegotiationResult::Negotiating {
                        proposal: offer,
                        score,
                    });
                }
            }
        }

        Ok(NegotiationResult::Ready {
            proposal: offer,
            score,
        })
    }

    fn fill_template(
        &mut self,
        mut offer_template: OfferTemplate,
    ) -> anyhow::Result<OfferTemplate> {
        offer_template.set_property(
            DEBIT_NOTE_INTERVAL_PROPERTY_FLAT,
            serde_json::Value::Number(self.interval.num_seconds().into()),
        );
        Ok(offer_template)
    }
}

fn read_duration(pointer: &str, proposal: &ProposalView) -> anyhow::Result<Option<Duration>> {
    match proposal.pointer_typed::<u32>(pointer) {
        Ok(val) => Ok(Some(Duration::seconds(val as i64))),
        Err(Error::NoKey { .. }) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<NegotiatorConfig> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        let config = Config::from_iter_safe(&[""])?;
        Ok(NegotiatorConfig {
            name: "DebitNoteInterval".to_string(),
            load_mode: LoadMode::StaticLib {
                library: "ya-provider".to_string(),
            },
            params: serde_yaml::to_value(&config)?,
        })
    }
}
