use metrics_core::{Builder, Drain, Observe, Observer};

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
        observer.drain()
    }
}
