use chrono::{DateTime, Utc};
use futures::channel::mpsc;
use serde::Serialize;
use std::collections::BTreeMap as Map;
use tokio::sync::broadcast;
use ya_core_model::market::Agreement;

#[derive(Serialize)]
struct TrackingEvent {
    ts: DateTime<Utc>,
    activities: Vec<ActivityStateModel>,
}

impl TrackingEvent {
    fn new() -> Self {
        let ts = Utc::now();
        let activities = Vec::new();
        TrackingEvent { ts, activities }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityStateModel {
    id: String,
    usage: Map<String, f64>,
    exe_unit: Option<String>,
}

pub enum Command {
    Start {
        activity_id: String,
        agreement_id: Option<String>,
        agreement: Option<Agreement>,
    },
    Stop {
        activity_id: String,
    },
}

#[derive(Clone)]
pub struct TrackerRef {
    tx: mpsc::UnboundedSender<Command>,
}

pub fn start_tracker() -> (TrackerRef, broadcast::Receiver<TrackingEvent>) {
    let (tx_event, rx_event) = broadcast::channel(1);
    let (tx, rx) = mpsc::unbounded();
    tokio::spawn(async move {
        while let Some(command) = rx.recv().await {
            match command {
                Command::Start {
                    activity_id,
                    agreement_id,
                    agreement,
                } => {
                    let _ = tx_event.send(TrackingEvent::new());
                    todo!();
                }
                Command::Stop { .. } => {
                    todo!()
                }
            }
        }
    });

    (TrackerRef { tx }, rx_event)
}
