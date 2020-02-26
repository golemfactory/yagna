use std::time::Duration;

/* TODO */

enum WasmRuntime {
    Wasi(i32), /* Wasi version */
}

struct ImageSpec {
    runtime: WasmRuntime,
}

impl ImageSpec {
    fn from_github<T: Into<String>>(_github_repository: T) -> Self {
        Self {
            runtime: WasmRuntime::Wasi(1),
        }
    }
    fn runtime(&mut self, runtime: WasmRuntime) {
        self.runtime = runtime
    }
}

struct TaskSession {
    name: String,
    timeout: Duration,
    // TODO demand: WasmDemand,
}

impl TaskSession {
    fn new() -> TaskSession {
        Self {
            name: "".into(),
            timeout: Duration::from_secs(60),
        }
    }
    fn with_timeout(mut self, duration: std::time::Duration) -> Self {
        self.timeout = duration;
        self
    }
    fn demand(self) -> Self {
        self
    }
    fn run() {}
}

struct WasmDemand {
    spec: ImageSpec,
    min_ram_gib: f32,
    min_storage_gib: f32,
}

impl WasmDemand {
    fn with_image(spec: ImageSpec) -> Self {
        Self {
            spec,
            min_ram_gib: 0.0,
            min_storage_gib: 0.0,
        }
    }
    fn min_ram_gib(mut self, min_ram_gib: f32) -> Self {
        self.min_ram_gib = min_ram_gib;
        self
    }
    fn min_storage_gib(mut self, min_storage_gib: f32) -> Self {
        self.min_storage_gib = min_storage_gib;
        self
    }
}
