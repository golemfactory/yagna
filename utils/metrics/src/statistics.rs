use futures::lock::Mutex;
use std::sync::Arc;

use crate::exporter::StringExporter;
use metrics_runtime::{observers::PrometheusBuilder, Controller, Receiver, Sink};

pub struct Statistics {
    //pub receiver: Receiver,
    pub root_sink: Sink,
    pub exporter: StringExporter<Controller, PrometheusBuilder>,
}

impl Statistics {
    pub fn new() -> Result<Arc<Mutex<Statistics>>, anyhow::Error> {
        let receiver = Receiver::builder().build()?;
        let sink = receiver.sink();

        let exporter = StringExporter::new(receiver.controller(), PrometheusBuilder::new());

        receiver.install();
        let stats = Statistics {
            //receiver,
            root_sink: sink,
            exporter,
        };
        return Ok(Arc::new(Mutex::new(stats)));
    }

    pub fn create_sink(&mut self, name: &str) -> std::sync::Mutex<Sink> {
        std::sync::Mutex::new(self.root_sink.scoped(name))
    }

    pub fn query_metrics(&mut self) -> String {
        return self.exporter.turn();
    }
}
