use futures::lock::Mutex;
use metrics_runtime::{observers::PrometheusBuilder, Controller, Receiver, Sink};
use std::sync::Arc;

use crate::exporter::StringExporter;

pub struct Metrics {
    //pub receiver: Receiver,
    pub root_sink: Sink,
    pub exporter: StringExporter<Controller, PrometheusBuilder>,
}

impl Metrics {
    pub fn new() -> Arc<Mutex<Metrics>> {
        let receiver = Receiver::builder()
            .build()
            .expect("Metrics initialization failure");
        let root_sink = receiver.sink();
        let exporter = StringExporter::new(receiver.controller(), PrometheusBuilder::new());
        receiver.install();

        Arc::new(Mutex::new(Self {
            //receiver,
            root_sink,
            exporter,
        }))
    }

    #[allow(dead_code)]
    pub fn create_sink(&mut self, name: &str) -> std::sync::Mutex<Sink> {
        std::sync::Mutex::new(self.root_sink.scoped(name))
    }

    pub fn export(&mut self) -> String {
        return self.exporter.turn();
    }
}
