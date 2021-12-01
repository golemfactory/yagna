use metrics_core::{Builder, Drain, Key, Observe, Observer};
use std::collections::{BTreeMap};
use metrics_runtime::Controller;
use metrics_runtime::observers::PrometheusBuilder;

pub struct CustomMetricsExporter {
    controller : Controller,
    json_builder : JsonBuilder,
    prometheus_builder : PrometheusBuilder,
}

impl CustomMetricsExporter
{
    pub fn new(controller: Controller, json_builder: JsonBuilder, prometheus_builder: PrometheusBuilder) -> Self {
        CustomMetricsExporter {
            controller,
            json_builder,
            prometheus_builder
        }
    }

    pub fn turn_json(&mut self) -> String {
        let mut observer = self.json_builder.build();
        self.controller.observe(&mut observer);
        return observer.drain();
    }

    pub fn turn_prometheus(&mut self) -> String {
        let mut observer = self.prometheus_builder.build();
        self.controller.observe(&mut observer);
        return observer.drain();
    }

}

pub struct JsonBuilder {
    pretty: bool,
}

impl JsonBuilder {
    pub fn new() -> Self {
        Self {
            pretty: true,
        }
    }
}

impl Builder for JsonBuilder {
    type Output = JsonObserver;

    fn build(&self) -> Self::Output {
        JsonObserver {
            pretty: self.pretty,
            tree: BTreeMap::new(),
        }
    }
}

impl Default for JsonBuilder {
    fn default() -> Self {
        Self::new()
    }
}


/// Observes metrics in JSON format.
pub struct JsonObserver {
    pub pretty: bool,
    pub tree: BTreeMap<String, i64>,
}

impl Observer for JsonObserver {
    fn observe_counter(&mut self, key: Key, value: u64) {
        self.tree.insert(key.name().to_string(), value as i64);
    }

    fn observe_gauge(&mut self, key: Key, value: i64) {
        self.tree.insert(key.name().to_string(), value as i64);
    }

    fn observe_histogram(&mut self, _key: Key, _values: &[u64]) {
        //no need to
    }
}

impl Drain<String> for JsonObserver {
    fn drain(&mut self) -> String {
        let result = if self.pretty {
            serde_json::to_string_pretty(&self.tree)
        } else {
            serde_json::to_string(&self.tree)
        };
        let rendered = result.expect("failed to render json output");
        self.tree.clear();
        rendered
    }
}
