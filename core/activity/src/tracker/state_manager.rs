#![deny(missing_docs)]

use super::{ActivityStateModel, Map, TrackingEvent};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::broadcast;
use ya_client_model::activity::State;
use ya_core_model::NodeId;

struct ExeUnitStatus {
    activity_id: String,
    identity_id: NodeId,
    agreement_id: String,
    exe_unit: Option<Arc<str>>,
    counters: Vec<Arc<str>>,
    last_state: State,
    values: Option<Vec<f64>>,
}

impl ExeUnitStatus {
    fn usage(&self) -> Option<Map<String, f64>> {
        self.values.as_ref().map(|values| {
            self.counters
                .iter()
                .zip(values)
                .map(|(counter, value)| (String::from(counter.as_ref()), *value))
                .collect()
        })
    }
}

pub struct StateManager {
    events: broadcast::Sender<TrackingEvent>,
    states: Map<Box<str>, ExeUnitStatus>,
}

impl StateManager {
    pub fn new(events: broadcast::Sender<TrackingEvent>) -> Self {
        let states = Default::default();
        Self { events, states }
    }

    pub fn start_activity(
        &mut self,
        activity_id: String,
        identity_id: NodeId,
        agreement_id: String,
        exe_unit: Option<Arc<str>>,
        counters: Vec<Arc<str>>,
    ) {
        let _ = self.states.insert(
            Box::from(activity_id.as_str()),
            ExeUnitStatus {
                activity_id,
                identity_id,
                agreement_id,
                exe_unit,
                counters,
                last_state: State::New,
                values: None,
            },
        );
    }

    pub fn update_counters(&mut self, activity_id: &str, counters: Vec<f64>) -> bool {
        if let Some(state) = self.states.get_mut(activity_id) {
            state.values = Some(counters);
            true
        } else {
            false
        }
    }

    pub fn update_state(&mut self, activity_id: &str, new_state: State) -> bool {
        if let Some(state) = self.states.get_mut(activity_id) {
            if new_state != state.last_state {
                state.last_state = new_state;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn destroy_activity(&mut self, activity_id: &str) -> bool {
        self.states.remove(activity_id).is_some()
    }

    pub fn subscribe(&self) -> (TrackingEvent, broadcast::Receiver<TrackingEvent>) {
        (self.current_state(), self.events.subscribe())
    }

    fn current_state(&self) -> TrackingEvent {
        TrackingEvent {
            ts: Utc::now(),
            activities: self
                .states
                .values()
                .map(|state| ActivityStateModel {
                    id: String::from(state.activity_id.as_str()),
                    agreement_id: state.agreement_id.clone(),
                    state: state.last_state,
                    usage: state.usage(),
                    exe_unit: state.exe_unit.as_ref().map(|v| v.to_string()),
                    provider_id: Some(state.identity_id),
                })
                .collect(),
        }
    }

    pub fn emit_state(&self) {
        match self.events.send(self.current_state()) {
            Ok(cnt) => log::trace!("send to {} recievers", cnt),
            Err(_) => (),
        }
    }
}
