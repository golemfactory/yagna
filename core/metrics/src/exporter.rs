use metrics_core::{Builder, Drain, Key, Label, Observe, Observer};
use std::collections::{BTreeMap};
use metrics_runtime::Controller;
use metrics_runtime::observers::PrometheusBuilder;

/// Exports metrics by converting them to a textual representation and logging them.
pub struct StringExporter<C, B>
where
    B: Builder,
{
    controller: C,
    builder: B,
}

impl<C, B> StringExporter<C, B>
where
    B: Builder,
    B::Output: Drain<String> + Observer,
    C: Observe,
{
    /// Creates a new [`StringExporter`] that logs at the configurable level.
    ///
    /// Observers expose their output by being converted into strings.
    pub fn new(controller: C, builder: B) -> Self {
        StringExporter {
            controller,
            builder,
        }
    }

    /// Run this exporter, logging output only once.
    pub fn turn(&mut self) -> String {
        let mut observer = self.builder.build();
        self.controller.observe(&mut observer);
        return observer.drain();
    }
}

pub struct JsonExporter {
    controller : Controller,
    builder : JsonBuilder,
    builder2 : PrometheusBuilder,
}

impl JsonExporter
{
    pub fn new(controller: Controller, builder: JsonBuilder, builder2: PrometheusBuilder) -> Self {
        JsonExporter {
            controller,
            builder,
            builder2
        }
    }

    pub fn turn(&mut self) -> String {
        let mut observer = self.builder.build();
        self.controller.observe(&mut observer);
        return observer.drain();
    }

    pub fn turn_prometheus(&mut self) -> String {
        let mut observer = self.builder2.build();
        self.controller.observe(&mut observer);
        return observer.drain();
    }

}

/// Builder for [`JsonObserver`].
pub struct JsonBuilder {
    pretty: bool,
}

impl JsonBuilder {
    /// Creates a new [`JsonBuilder`] with default values.
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
