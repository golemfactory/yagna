use futures::lock::Mutex;
use metrics_runtime::{observers::PrometheusBuilder, Controller, Receiver, Sink};
use std::sync::Arc;
use std::collections::{HashMap};

use crate::exporter::{StringExporter, JsonBuilder, JsonExporter};
use metrics_core::Drain;

pub struct Metrics {
    //pub receiver: Receiver,
    pub root_sink: Sink,
   // pub exporter: StringExporter<Controller, PrometheusBuilder>,
    pub json_exporter: JsonExporter,
}

impl Metrics {
    pub fn new() -> Arc<Mutex<Metrics>> {
        let receiver = Receiver::builder()
            .build()
            .expect("Metrics initialization failure");
       // let root_sink = receiver.sink();
        //let exporter = StringExporter::new(receiver.controller(), PrometheusBuilder::new());

      //  receiver.install();
        let receiver2 = Receiver::builder()
            .build()
            .expect("Metrics initialization failure");
        let root_sink = receiver2.sink();

        let json_exporter = JsonExporter::new(receiver2.controller(), JsonBuilder::new(), PrometheusBuilder::new());
        receiver2.install();

        Arc::new(Mutex::new(Self {
            //receiver,
            root_sink,
       //     exporter,
            json_exporter
        }))
    }

    #[allow(dead_code)]
    pub fn create_sink(&mut self, name: &str) -> std::sync::Mutex<Sink> {
        std::sync::Mutex::new(self.root_sink.scoped(name))
    }

    pub fn export(&mut self, json: bool) -> String {
        if json {
            self.json_exporter.turn()
        }
        else {
            self.json_exporter.turn_prometheus()
        }
    }


}
