#![allow(dead_code)]

pub static POC_OFFER_PROPERTIES_JSON: &str = r#"
{
  "golem.com.pricing.model": "linear",
  "golem.com.pricing.model.linear.coeffs": [
    0.1,
    0.2,
    1.0
  ],
  "golem.com.scheme": "payu",
  "golem.com.scheme.payu.interval_sec": 6.0,
  "golem.com.usage.vector": [
    "golem.usage.duration_sec",
    "golem.usage.cpu_sec"
  ],
  "golem.inf.mem.gib": 1.0,
  "golem.inf.storage.gib": 10.0,
  "golem.node.debug.subnet": "piotr",
  "golem.node.id.name": "2rec-prov@dan",
  "golem.runtime.name": "wasmtime",
  "golem.runtime.version": "0.1.0",
  "golem.runtime.wasm.wasi.version@v": "0.9.0"
}"#;

pub static POC_OFFER_PROPERTIES_JSON_DEEP: &str = r#"
{
  "golem": {
    "com": {
      "pricing": {
        "model": "linear",
        "model.linear": {
          "coeffs": [
            0.1,
            0.2,
            1.0
          ]
        }
      },
      "scheme": "payu",
      "scheme.payu": {
        "interval_sec": 6.0
      },
      "usage": {
        "vector": [
          "golem.usage.duration_sec",
          "golem.usage.cpu_sec"
        ]
      }
    },
    "inf": {
      "mem": {
        "gib": 1.0
      },
      "storage": {
        "gib": 10.0
      }
    },
    "node": {
      "debug": {
        "subnet": "piotr"
      },
      "id": {
        "name": "2rec-prov@dan"
      }
    },
    "runtime": {
      "name": "wasmtime",
      "version": "0.1.0",
      "wasm.wasi.version@v": "0.9.0"
    }
  }
}"#;

pub static POC_OFFER_PROPERTIES_FLAT: &'static [&'static str] = &[
    "golem.com.pricing.model=\"linear\"",
    "golem.com.pricing.model.linear.coeffs=[0.1,0.2,1.0]",
    "golem.com.scheme=\"payu\"",
    "golem.com.scheme.payu.interval_sec=6.0",
    "golem.com.usage.vector=[\"golem.usage.duration_sec\",\"golem.usage.cpu_sec\"]",
    "golem.inf.mem.gib=1.0",
    "golem.inf.storage.gib=10.0",
    "golem.node.debug.subnet=\"piotr\"",
    "golem.node.id.name=\"2rec-prov@dan\"",
    "golem.runtime.name=\"wasmtime\"",
    "golem.runtime.version=\"0.1.0\"",
    "golem.runtime.wasm.wasi.version@v=\"0.9.0\"",
];

pub static POC_DEMAND_PROPERTIES_JSON: &str = r#"
{
  "golem.node.debug.subnet": "piotr",
  "golem.node.id.name": "test1",
  "golem.srv.comp.expiration": 1590765503361,
  "golem.srv.comp.task_package": "hash://sha3:D5E31B2EED628572A5898BF8C34447644BFC4B5130CFC1E4F10AEAA1:http://12.34.56.78:8000/rust-wasi-tutorial.zip"
}"#;

pub static POC_DEMAND_PROPERTIES_JSON_DEEP: &str = r#"
{
  "golem": {
    "node": {
      "debug.subnet": "piotr",
      "id.name": "test1"
    },
    "srv.comp": {
      "expiration": 1590765503361,
      "task_package": "hash://sha3:D5E31B2EED628572A5898BF8C34447644BFC4B5130CFC1E4F10AEAA1:http://12.34.56.78:8000/rust-wasi-tutorial.zip"
    }
  }
}"#;

pub static POC_DEMAND_PROPERTIES_FLAT: &'static [&'static str] = &[
    "golem.node.debug.subnet=\"piotr\"",
    "golem.node.id.name=\"test1\"",
    "golem.srv.comp.expiration=1590765503361",
    "golem.srv.comp.task_package=\"hash://sha3:D5E31B2EED628572A5898BF8C34447644BFC4B5130CFC1E4F10AEAA1:http://12.34.56.78:8000/rust-wasi-tutorial.zip\""
];

pub static POC_DEMAND_CONSTRAINTS: &'static str = r#"
(&
(golem.inf.mem.gib>0.5)
(golem.inf.storage.gib>1)
(golem.com.pricing.model=linear)
(golem.node.debug.subnet=piotr)
)"#;

pub static POC_OFFER_CONSTRAINTS: &'static str =
    "(&(golem.node.debug.subnet=piotr)(golem.srv.comp.expiration>0))";
