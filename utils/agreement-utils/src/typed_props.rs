use crate::OfferTemplate;
use serde_json::Value;

pub trait OfferBuilder {
    fn build(&self) -> Value;
}

#[derive(Clone)]
pub struct OfferDefinition {
    pub node_info: NodeInfo,
    pub srv_info: ServiceInfo,
    pub com_info: ComInfo,
    pub offer: OfferTemplate,
}

impl OfferDefinition {
    pub fn into_json(self) -> Value {
        let mut base = serde_json::Map::new();
        self.node_info.write_json(&mut base);
        self.srv_info.write_json(&mut base);
        self.com_info.write_json(&mut base);

        let template = OfferTemplate::new(serde_json::json!({ "golem": base }));
        template.patch(self.offer).properties
    }
}

#[derive(Clone)]
pub struct NodeInfo {
    pub name: Option<String>,
    pub subnet: Option<String>,
    pub geo_country_code: Option<String>,
}

impl NodeInfo {
    pub fn with_name(name: impl Into<String>) -> Self {
        NodeInfo {
            name: Some(name.into()),
            geo_country_code: None,
            subnet: None,
        }
    }

    pub fn with_subnet(&mut self, subnet: String) -> &mut Self {
        self.subnet = Some(subnet);
        self
    }

    fn write_json(self, map: &mut serde_json::Map<String, Value>) {
        let mut node = serde_json::Map::new();
        if let Some(name) = self.name {
            let _ = node.insert("id".into(), serde_json::json!({ "name": name }));
        }
        if let Some(cc) = self.geo_country_code {
            let _ = node.insert("geo".into(), serde_json::json!({ "country_code": cc }));
        }
        if let Some(subnet) = self.subnet {
            let _ = node.insert("debug".into(), serde_json::json!({ "subnet": subnet }));
        }
        map.insert("node".into(), node.into());
    }
}

#[derive(Clone)]
pub struct ServiceInfo {
    inf: InfNodeInfo,
    exeunit_info: Value,
    multi_activity: bool,
}

impl ServiceInfo {
    pub fn new(inf: InfNodeInfo, exeunit_info: Value) -> ServiceInfo {
        ServiceInfo {
            inf,
            exeunit_info,
            multi_activity: true,
        }
    }

    pub fn support_multi_activity(self, multi_activity: bool) -> Self {
        Self {
            multi_activity,
            ..self
        }
    }

    fn write_json(self, map: &mut serde_json::Map<String, Value>) {
        self.inf.write_json(map);
        let _ = map.insert("runtime".into(), self.exeunit_info);

        let srv_map = serde_json::json!({ "caps": {"multi-activity": self.multi_activity}});
        let _ = map.insert("srv".into(), srv_map);
    }
}

#[derive(Default, Clone)]
pub struct InfNodeInfo {
    mem_gib: Option<f64>,
    storage_gib: Option<f64>,
    cpu_info: Option<CpuInfo>,
}

impl InfNodeInfo {
    #[deprecated(note = "Please use Default::default instead")]
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

    pub fn with_cpu(self, cpu_info: CpuInfo) -> Self {
        Self {
            cpu_info: Some(cpu_info),
            ..self
        }
    }

    fn write_json(self, map: &mut serde_json::Map<String, Value>) {
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

    fn write_json(self, map: &mut serde_json::Map<String, Value>) {
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
    pub params: Value,
}

impl ComInfo {
    fn write_json(self, map: &mut serde_json::Map<String, Value>) {
        let _ = map.insert("com".to_string(), self.params.clone());
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
            srv_info: ServiceInfo {
                inf: InfNodeInfo::default().with_mem(5.0).with_storage(50.0),
                exeunit_info: serde_json::json!({"wasm.wasi.version@v".to_string(): "0.9.0".to_string()}),
                multi_activity: false,
            },
            com_info: Default::default(),
            offer: OfferTemplate::default(),
        };

        eprintln!(
            "j={}",
            serde_json::to_string_pretty(&offer.into_json()).unwrap()
        );
    }
}
