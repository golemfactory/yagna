use futures::lock::Mutex;
use metrics_runtime::{observers::PrometheusBuilder, Receiver, Sink};
use std::sync::Arc;

use crate::exporter::{JsonBuilder, CustomMetricsExporter};

pub struct Metrics {
    pub root_sink: Sink,
    pub exporter: CustomMetricsExporter,
}

impl Metrics {
    pub fn new() -> Arc<Mutex<Metrics>> {
        let receiver = Receiver::builder()
            .build()
            .expect("Metrics initialization failure");
        let root_sink = receiver.sink();
        let exporter = CustomMetricsExporter::new(receiver.controller(), JsonBuilder::new(), PrometheusBuilder::new());

        receiver.install();

        Arc::new(Mutex::new(Self {
            root_sink,
            exporter
        }))
    }

    #[allow(dead_code)]
    pub fn create_sink(&mut self, name: &str) -> std::sync::Mutex<Sink> {
        std::sync::Mutex::new(self.root_sink.scoped(name))
    }

    pub fn export(&mut self, json: bool) -> String {
        if json {
            self.exporter.turn_json()
        }
        else {
            self.exporter.turn_prometheus()
        }
    }
}
