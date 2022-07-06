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
        let exporter = StringExporter::new(
            receiver.controller(),
            PrometheusBuilder::new().set_quantiles(&[
                0.0, 0.01, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.99, 0.999,
            ]),
        );
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
