use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use structopt::StructOpt;

use ya_agreement_utils::{AgreementView, ProposalView};
use ya_negotiators::component::{RejectReason, Score};
use ya_negotiators::factory::{LoadMode, NegotiatorConfig};
use ya_negotiators::{AgreementResult, NegotiationResult, NegotiatorComponent};

/// Configuration for LimitAgreements Negotiator.
#[derive(StructOpt, Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[structopt(long, env, default_value = "1")]
    pub max_simultaneous_agreements: u32,
}

/// Negotiator that can limit number of running agreements.
pub struct MaxAgreements {
    active_agreements: HashSet<String>,
    max_agreements: u32,
}

impl MaxAgreements {
    pub fn new(config: serde_yaml::Value) -> anyhow::Result<MaxAgreements> {
        let config: Config = serde_yaml::from_value(config)?;
        Ok(MaxAgreements {
            max_agreements: config.max_simultaneous_agreements,
            active_agreements: HashSet::new(),
        })
    }

    pub fn has_free_slot(&self) -> bool {
        self.active_agreements.len() < self.max_agreements as usize
    }
}

impl NegotiatorComponent for MaxAgreements {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        offer: ProposalView,
        score: Score,
    ) -> anyhow::Result<NegotiationResult> {
        if self.has_free_slot() {
            Ok(NegotiationResult::Ready {
                proposal: offer,
                score,
            })
        } else {
            log::info!(
                "'MaxAgreements' negotiator: Reject proposal [{}] due to limit.",
                demand.id,
            );
            Ok(NegotiationResult::Reject {
                reason: RejectReason::new(format!(
                    "No capacity available. Reached Agreements limit: {}",
                    self.max_agreements
                )),
                is_final: false,
            })
        }
    }

    fn on_agreement_terminated(
        &mut self,
        agreement_id: &str,
        _result: &AgreementResult,
    ) -> anyhow::Result<()> {
        self.active_agreements.remove(agreement_id);

        let free_slots = self.max_agreements as usize - self.active_agreements.len();
        log::info!("Negotiator: {} free slot(s) for agreements.", free_slots);
        Ok(())
    }

    fn on_agreement_approved(&mut self, agreement: &AgreementView) -> anyhow::Result<()> {
        if self.has_free_slot() {
            self.active_agreements.insert(agreement.id.to_string());
            Ok(())
        } else {
            self.active_agreements.insert(agreement.id.to_string());
            bail!(
                "Agreement [{}] approved despite not available capacity.",
                agreement.id
            )
        }
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<NegotiatorConfig> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        let config = Config::from_iter_safe(&[""])?;
        Ok(NegotiatorConfig {
            name: "LimitAgreements".to_string(),
            load_mode: LoadMode::StaticLib {
                library: "ya-provider".to_string(),
            },
            params: serde_yaml::to_value(&config)?,
        })
    }
}
