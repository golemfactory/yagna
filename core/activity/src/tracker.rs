use chrono::{DateTime, Utc};
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::prelude::*;
use serde::Serialize;
use std::collections::BTreeMap as Map;
use std::sync::Arc;
use tokio::sync::broadcast;
use ya_client_model::activity::activity_state::ActivityState;

mod name_pool;
mod state_manager;

use anyhow::Context;
use name_pool::NamePool;
use ya_core_model::market::Agreement;

#[derive(Serialize)]
pub struct TrackingEvent {
    ts: DateTime<Utc>,
    activities: Vec<ActivityStateModel>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStateModel {
    id: String,
    state: ActivityState,
    usage: Option<Map<String, f64>>,
    exe_unit: Option<String>,
}

pub enum Command {
    Start {
        activity_id: Arc<str>,
        agreement_id: Arc<str>,
        exe_unit: Option<String>,
        counters: Vec<String>,
    },
    Stop {
        activity_id: String,
    },
    Subscribe {
        tx: oneshot::Sender<(TrackingEvent, broadcast::Receiver<TrackingEvent>)>,
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
        let agreement_id = Arc::<str>::from(agreement.agreement_id.as_str());
        let activity_id: Arc<str> = activity_id.into();

        fn extract_agreement(
            agreement: &Agreement,
        ) -> anyhow::Result<(Option<String>, Vec<String>)> {
            if let Some(obj) = agreement.offer.properties.as_object() {
                let exe_init = obj
                    .get("golem.runtime.name")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let counters = if let Some(counters_json) = obj.get("golem.com.usage.vector") {
                    serde_json::from_value(counters_json.clone())?
                } else {
                    Vec::new()
                };
                return Ok((exe_init, counters));
            }
            anyhow::bail!("invalid agreement format");
        }
        let (exe_unit, counters) = extract_agreement(agreement)?;
        self.tx
            .send(Command::Start {
                activity_id,
                agreement_id,
                exe_unit,
                counters,
            })
            .await
            .context("track start activity")
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
                    agreement_id,
                    exe_unit,
                    counters,
                } => {
                    let exe_unit = exe_unit.map(|s| exe_units_names.alloc(&s));
                    let counters = counters
                        .into_iter()
                        .map(|counter| exe_units_names.alloc(&counter))
                        .collect();

                    exe_unit_states.start_activity(activity_id, agreement_id, exe_unit, counters);
                    exe_unit_states.emit_state();
                }
                Command::Stop { activity_id } => {
                    exe_unit_states.destroy_activity(&activity_id);
                    exe_unit_states.emit_state();
                }
                Command::Subscribe { tx } => {
                    tx.send(exe_unit_states.subscribe());
                }
            }
        }
    });

    (TrackerRef { tx }, rx_event)
}
