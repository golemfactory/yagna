use chrono::{DateTime, Utc};
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap as Map;
use tokio::sync::broadcast;

mod name_pool;
mod state_manager;

use anyhow::Context;
use name_pool::NamePool;
use ya_client_model::activity::State;
use ya_core_model::market::Agreement;
use ya_core_model::NodeId;

#[derive(Serialize, Clone)]
pub struct TrackingEvent {
    ts: DateTime<Utc>,
    activities: Vec<ActivityStateModel>,
}

impl TrackingEvent {
    pub fn for_provider(self, provider_id: NodeId) -> Self {
        Self {
            ts: self.ts,
            activities: self
                .activities
                .into_iter()
                .filter_map(|mut state| {
                    if state.provider_id == Some(provider_id) {
                        state.provider_id = None;
                        Some(state)
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStateModel {
    id: String,
    state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<Map<String, f64>>,
    exe_unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_id: Option<NodeId>,
    agreement_id: String,
}

pub enum Command {
    Start {
        activity_id: String,
        identity_id: NodeId,
        agreement_id: String,
        exe_unit: Option<String>,
        counters: Vec<String>,
    },
    Stop {
        activity_id: String,
    },
    Subscribe {
        tx: oneshot::Sender<(TrackingEvent, broadcast::Receiver<TrackingEvent>)>,
    },
    UpdateState {
        activity_id: String,
        state: State,
    },
    UpdateCounters {
        activity_id: String,
        counters: Vec<f64>,
    },
}

#[derive(Clone)]
pub struct TrackerRef {
    tx: mpsc::UnboundedSender<Command>,
}

impl TrackerRef {
    pub fn create() -> Self {
        start_tracker().0
    }

    pub async fn subscribe(
        &mut self,
    ) -> anyhow::Result<(TrackingEvent, broadcast::Receiver<TrackingEvent>)> {
        let (tx, rx) = oneshot::channel();
        if let Ok(_) = self.tx.send(Command::Subscribe { tx }).await {
            return Ok(rx.await?);
        }
        anyhow::bail!("Fatal error activity state tracker is unavailable");
    }

    pub async fn start_activity(
        &mut self,
        activity_id: &str,
        agreement: &Agreement,
    ) -> anyhow::Result<()> {
        let activity_id: String = activity_id.into();

        fn extract_agreement(
            agreement: &Agreement,
        ) -> anyhow::Result<(Option<String>, Vec<String>, NodeId, String)> {
            if let Some(obj) = agreement.offer.properties.as_object() {
                let exe_init = obj
                    .get("golem.runtime.name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                if exe_init.is_none() {
                    log::error!("agr={:?}", &agreement.offer.properties);
                }
                let counters = if let Some(counters_json) = obj.get("golem.com.usage.vector") {
                    serde_json::from_value(counters_json.clone())?
                } else {
                    Vec::new()
                };
                return Ok((
                    exe_init,
                    counters,
                    agreement.offer.provider_id,
                    agreement.agreement_id.clone(),
                ));
            }
            anyhow::bail!("invalid agreement format");
        }
        let (exe_unit, counters, identity_id, agreement_id) = extract_agreement(agreement)?;
        self.tx
            .send(Command::Start {
                activity_id,
                identity_id,
                agreement_id,
                exe_unit,
                counters,
            })
            .await
            .context("track start activity")
    }

    pub async fn stop_activity(&mut self, activity_id: String) -> anyhow::Result<()> {
        self.tx
            .send(Command::Stop { activity_id })
            .await
            .context("track stop activity")
    }

    pub async fn update_state(&mut self, activity_id: String, state: State) -> anyhow::Result<()> {
        self.tx
            .send(Command::UpdateState { activity_id, state })
            .await
            .context("track new activity state")
    }

    pub async fn update_counters(
        &mut self,
        activity_id: String,
        counters: Vec<f64>,
    ) -> anyhow::Result<()> {
        self.tx
            .send(Command::UpdateCounters {
                activity_id,
                counters,
            })
            .await
            .context("track new activity counters")
    }
}

pub fn start_tracker() -> (TrackerRef, broadcast::Receiver<TrackingEvent>) {
    let (tx_event, rx_event) = broadcast::channel(1);
    let (tx, mut rx) = mpsc::unbounded();

    let mut exe_units_names = NamePool::default();
    let mut exe_unit_states = state_manager::StateManager::new(tx_event);

    tokio::spawn(async move {
        while let Some(command) = rx.next().await {
            match command {
                Command::Start {
                    activity_id,
                    identity_id,
                    agreement_id,
                    exe_unit,
                    counters,
                } => {
                    let exe_unit = exe_unit.map(|s| exe_units_names.alloc(&s));
                    let counters = counters
                        .into_iter()
                        .map(|counter| exe_units_names.alloc(&counter))
                        .collect();

                    exe_unit_states.start_activity(
                        activity_id,
                        identity_id,
                        agreement_id,
                        exe_unit,
                        counters,
                    );
                    exe_unit_states.emit_state();
                }
                Command::Stop { activity_id } => {
                    exe_unit_states.destroy_activity(&activity_id);
                    exe_unit_states.emit_state();
                }
                Command::Subscribe { tx } => {
                    if let Err(_) = tx.send(exe_unit_states.subscribe()) {
                        log::debug!("dead subscribe");
                    }
                }
                Command::UpdateState { activity_id, state } => {
                    if exe_unit_states.update_state(&activity_id, state) {
                        exe_unit_states.emit_state();
                    }
                }
                Command::UpdateCounters {
                    activity_id,
                    counters,
                } => {
                    if exe_unit_states.update_counters(&activity_id, counters) {
                        exe_unit_states.emit_state();
                    }
                }
            }
        }
    });

    (TrackerRef { tx }, rx_event)
}
