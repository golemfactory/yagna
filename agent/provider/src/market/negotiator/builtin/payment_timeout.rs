use chrono::{DateTime, Duration, NaiveDateTime, Utc};
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

const PAYMENT_TIMEOUT_PROPERTY_FLAT: &str = "golem.com.scheme.payu.payment-timeout-sec?";
pub const PAYMENT_TIMEOUT_PROPERTY: &str = "/golem/com/scheme/payu/payment-timeout-sec?";
const EXPIRATION_PROPERTY: &str = "/golem/srv/comp/expiration";

/// PaymentTimeout negotiator
pub struct PaymentTimeout {
    min_timeout: Duration,
    max_timeout: Duration,
    timeout: Duration,
    required_from: Duration,
}

/// Configuration for PaymentTimeout negotiator
#[derive(StructOpt, Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "1s")]
    pub min_payment_timeout: std::time::Duration,
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "24h")]
    pub max_payment_timeout: std::time::Duration,
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "2min")]
    pub payment_timeout: std::time::Duration,
    #[serde(with = "humantime_serde")]
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "10h")]
    pub payment_timeout_required_duration: std::time::Duration,
}

impl NegotiatorFactory<PaymentTimeout> for PaymentTimeout {
    type Type = NegotiatorMut;

    fn new(
        _name: &str,
        config: serde_yaml::Value,
        _agent_env: serde_yaml::Value,
        _workdir: PathBuf,
    ) -> anyhow::Result<PaymentTimeout> {
        let config: Config = serde_yaml::from_value(config)?;

        let min_timeout = Duration::from_std(config.min_payment_timeout)?;
        let max_timeout = Duration::from_std(config.max_payment_timeout)?;
        let timeout = Duration::from_std(config.payment_timeout)?;
        let required_from = Duration::from_std(config.payment_timeout_required_duration)?;

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
            required_from,
        })
    }
}

impl NegotiatorComponentMut for PaymentTimeout {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        mut offer: ProposalView,
        score: Score,
    ) -> anyhow::Result<NegotiationResult> {
        let offer_timeout = read_duration(PAYMENT_TIMEOUT_PROPERTY, &offer)?;
        let demand_timeout = read_duration(PAYMENT_TIMEOUT_PROPERTY, demand)?;
        let expires_at = read_utc_timestamp(EXPIRATION_PROPERTY, demand)?;

        let now = Utc::now();
        let allow_compat = if expires_at > now {
            (expires_at - now) < self.required_from
        } else {
            return Ok(NegotiationResult::Reject {
                reason: RejectReason::new(
                    "Computation expiration time was set in the past".to_string(),
                ),
                is_final: true,
            });
        };

        match demand_timeout {
            Some(timeout) => {
                let offer_timeout = offer_timeout.ok_or_else(|| {
                    anyhow::anyhow!("DebitNote payment timeout not found in the Offer")
                })?;

                if timeout < self.min_timeout || timeout > self.max_timeout {
                    return Ok(NegotiationResult::Reject {
                        reason: RejectReason::new(format!(
                            "Demand DebitNote payment timeout {} not in acceptable range of [{}; {}]",
                            timeout.display(),
                            self.min_timeout.display(),
                            self.max_timeout.display(),
                        )),
                        is_final: true,
                    });
                } else if offer_timeout != timeout {
                    let property = offer.pointer_mut(PAYMENT_TIMEOUT_PROPERTY).unwrap();
                    *property = serde_json::Value::Number(timeout.num_seconds().into());
                    return Ok(NegotiationResult::Negotiating {
                        proposal: offer,
                        score,
                    });
                }
            }
            None => {
                if !allow_compat {
                    return Ok(NegotiationResult::Reject {
                        reason: RejectReason::new(format!(
                            "Expiration time {} exceeds the {} threshold of enforcing mid-agreement payments \
                            but the required property '{}' was not present in the Demand",
                            expires_at.to_rfc3339(),
                            self.required_from.display(),
                            PAYMENT_TIMEOUT_PROPERTY_FLAT
                        )),
                        is_final: true,
                    });
                } else if offer_timeout.is_some() {
                    let _ = offer.remove_property(PAYMENT_TIMEOUT_PROPERTY);
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
            PAYMENT_TIMEOUT_PROPERTY_FLAT,
            serde_json::Value::Number(self.timeout.num_seconds().into()),
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

fn read_utc_timestamp(pointer: &str, proposal: &ProposalView) -> anyhow::Result<DateTime<Utc>> {
    match proposal.pointer_typed::<u64>(pointer) {
        Ok(val) => {
            let secs = (val / 1000) as i64;
            let nsecs = 1_000_000 * (val % 1000) as u32;
            let naive = NaiveDateTime::from_timestamp_opt(secs, nsecs)
                .ok_or_else(|| anyhow::anyhow!("Cannot make DateTime from {secs} and {nsecs}"))?;
            Ok(DateTime::from_utc(naive, Utc))
        }
        Err(err) => Err(err.into()),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<NegotiatorConfig> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        let config = Config::from_iter_safe(&[""])?;
        Ok(NegotiatorConfig {
            name: "PaymentTimeout".to_string(),
            load_mode: LoadMode::StaticLib {
                library: "ya-provider".to_string(),
            },
            params: serde_yaml::to_value(&config)?,
        })
    }
}
