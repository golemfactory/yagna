#[derive(Clone)]
pub struct OfferDefinition {
    pub node_info: NodeInfo,
    pub service: ServiceInfo,
    pub com_info: ComInfo,
}

impl OfferDefinition {
    pub fn into_json(self) -> serde_json::Value {
        let mut base = serde_json::Map::new();
        self.node_info.write_json(&mut base);
        self.service.write_json(&mut base);
        self.com_info.write_json(&mut base);
        serde_json::json!({ "golem": base })
    }
}

#[derive(Clone)]
pub struct NodeInfo {
    name: Option<String>,
    geo_country_code: Option<String>,
}

impl NodeInfo {
    pub fn with_name(name: impl Into<String>) -> Self {
        NodeInfo {
            name: Some(name.into()),
            geo_country_code: None,
        }
    }

    fn write_json(self, map: &mut serde_json::Map<String, serde_json::Value>) {
        let mut node = serde_json::Map::new();
        if let Some(name) = self.name {
            let _ = node.insert("id".into(), serde_json::json!({ "name": name }));
        }
        if let Some(cc) = self.geo_country_code {
            let _ = node.insert("geo".into(), serde_json::json!({ "country_code": cc }));
        }
        map.insert("node".into(), node.into());
    }
}

#[derive(Clone)]
pub enum ServiceInfo {
    Wasm {
        inf: InfNodeInfo,
        wasi_version: String,
    },
}

impl ServiceInfo {
    fn write_json(self, map: &mut serde_json::Map<String, serde_json::Value>) {
        match self {
            ServiceInfo::Wasm { inf, wasi_version } => {
                inf.write_json(map);
                let _ = map.insert(
                    "runtime".into(),
                    serde_json::json!({
                        "wasm":{
                            "wasi": {
                                "version@v": wasi_version
                            }
                        }
                    }),
                );
            }
        }
    }
}

#[derive(Default, Clone)]
pub struct InfNodeInfo {
    mem_gib: Option<f64>,
    storage_gib: Option<f64>,
    cpu_info: Option<CpuInfo>,
}

impl InfNodeInfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_mem(self, mem_gib: f64) -> Self {
        Self {
            mem_gib: Some(mem_gib),
            ..self
        }
    }

    pub fn with_storage(self, storage_gib: f64) -> Self {
        Self {
            storage_gib: Some(storage_gib),
            ..self
        }
    }

    fn write_json(self, map: &mut serde_json::Map<String, serde_json::Value>) {
        let mut inf_map = serde_json::Map::new();
        if let Some(mem) = self.mem_gib {
            let _ = inf_map.insert("mem".to_string(), serde_json::json!({ "gib": mem }));
        }
        if let Some(storage) = self.storage_gib {
            let _ = inf_map.insert("storage".to_string(), serde_json::json!({ "gib": storage }));
        }
        if let Some(cpu) = self.cpu_info {
            cpu.write_json(&mut inf_map);
        }
        let _ = map.insert("inf".to_string(), inf_map.into());
    }
}

// golem.inf.cpu.architecture
// golem.inf.cpu.cores
// golem.inf.cpu.threads

#[derive(Default, Clone)]
pub struct CpuInfo {
    pub architecture: String,
    pub cores: u32,
    pub threads: u32,
}

impl CpuInfo {
    pub fn for_wasm(cores: u32) -> Self {
        CpuInfo {
            architecture: "wasm32".to_string(),
            cores,
            threads: cores,
        }
    }

    fn write_json(self, map: &mut serde_json::Map<String, serde_json::Value>) {
        let _ = map.insert(
            "cpu".to_string(),
            serde_json::json!({
                "architecture": self.architecture,
                "cores": self.cores,
                "threads": self.threads
            }),
        );
    }
}

#[derive(Default, Clone)]
pub struct ComInfo {
    _inner: (),
}

impl ComInfo {
    fn write_json(self, _map: &mut serde_json::Map<String, serde_json::Value>) {
        // TODO:
    }
}

// golem.inf.mem.gib
// golem.inf.storage.gib
// R: golem.activity.timeout_secs

// golem.com.payment.scheme="payu"
// golem.com.payment.scheme.payu.interval_sec=3600
// golem.com.pricing.model="linear"
// golem.com.pricing.model.linear.coeffs=[0.3, 0]
// golem.usage.vector=["golem.usage.duration_sec"]

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_wasm_1() {
        let offer = OfferDefinition {
            node_info: NodeInfo::with_name("dany"),
            service: ServiceInfo::Wasm {
                inf: InfNodeInfo::default().with_mem(5.0).with_storage(50.0),
                wasi_version: "0.0".to_string(),
            },
            com_info: Default::default(),
        };

        eprintln!(
            "j={}",
            serde_json::to_string_pretty(&offer.into_json()).unwrap()
        );
    }
}
