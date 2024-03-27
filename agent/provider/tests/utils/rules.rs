use golem_certificate::schemas::certificate::Fingerprint;
use hex::ToHex;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use ya_agreement_utils::agreement::expand;
use ya_agreement_utils::{OfferTemplate, ProposalView};
use ya_client_model::market::proposal::State;
use ya_client_model::NodeId;
use ya_manifest_test_utils::TestResources;
use ya_provider::market::negotiator::NegotiationResult;
use ya_provider::rules::restrict::{RestrictRule, RuleAccessor};
use ya_provider::rules::RulesManager;

static MANIFEST_TEST_RESOURCES: TestResources = TestResources {
    temp_dir: env!("CARGO_TARGET_TMPDIR"),
};

pub fn init_certificates() -> PathBuf {
    let (resource_cert_dir, _test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();
    resource_cert_dir
}

pub fn setup_rules_manager() -> RulesManager {
    let (_resource_cert_dir, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    let whitelist_file = test_cert_dir.join("whitelist.json");
    let rules_file_name = test_cert_dir.join("rules.json");

    let mut rules_manager =
        RulesManager::load_or_create(&rules_file_name, &whitelist_file, &test_cert_dir)
            .expect("Can't load RulesManager");

    import_certificates(
        &mut rules_manager,
        &[
            "root-certificate.signed.json",
            "partner-certificate.signed.json",
            "independent-chain-depth-3.cert.signed.json",
            "independent-chain-depth-2.cert.signed.json",
            "independent-chain-depth-1.cert.signed.json",
            "root-cert-independent-chain.cert.signed.json",
        ],
    );

    rules_manager
}

pub fn import_certificates(rules: &mut RulesManager, certs: &[&str]) {
    let (resource_cert_dir, _test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    for cert in certs {
        let cert_path = resource_cert_dir.join(cert);
        rules.import_certs(&cert_path).unwrap();
    }
}

pub fn create_demand(demand: Value) -> ProposalView {
    ProposalView {
        content: OfferTemplate {
            properties: expand(demand),
            constraints: "()".to_string(),
        },
        id: "0x0000000000000000000000000000000000000000".to_string(),
        issuer: Default::default(),
        state: State::Initial,
        timestamp: Default::default(),
    }
}

pub fn create_offer() -> ProposalView {
    ProposalView {
        content: OfferTemplate {
            properties: expand(serde_json::from_str(r#"{ "any": "thing" }"#).unwrap()),
            constraints: "()".to_string(),
        },
        id: "0x0000000000000000000000000000000000000000".to_string(),
        issuer: Default::default(),
        state: State::Initial,
        timestamp: Default::default(),
    }
}

pub fn load_node_descriptor(file: Option<&str>) -> Value {
    let (resource_cert_dir, _test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    if let Some(file) = file {
        let data = std::fs::read(resource_cert_dir.join(file)).unwrap();
        let desc = serde_json::from_slice::<Value>(&data).unwrap();
        json!({
            "golem": {
                "!exp": {
                    "gap-31": {
                        "v0": {
                            "node": {
                                "descriptor": desc
                            }
                        }
                    }

                }
            },
        })
    } else {
        Value::Null
    }
}

pub fn fingerprint(input_file_path: &Path) -> anyhow::Result<Fingerprint> {
    let json_string = fs::read_to_string(input_file_path)?;
    let input_json: Value = serde_json::from_str(&json_string)?;

    let signed_data = &input_json["certificate"];
    let fingerprint = golem_certificate::create_default_hash(signed_data)?;
    Ok(fingerprint.encode_hex::<String>())
}

pub fn setup_certificates_rules<G: RuleAccessor>(rules: RestrictRule<G>, certs: &[&str]) {
    let (resource_cert_dir, _test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();
    for cert in certs {
        let path = resource_cert_dir.join(cert);
        let fingerprint = fingerprint(&path).unwrap();
        rules.add_certified_rule(&fingerprint).unwrap();
    }
}

pub fn setup_identity_rules<G: RuleAccessor>(rules: RestrictRule<G>, ids: &[&str]) {
    for id in ids {
        rules
            .add_identity_rule(NodeId::from_str(id).unwrap())
            .unwrap();
    }
}

pub fn expect_accept(result: NegotiationResult) {
    match result {
        NegotiationResult::Ready { .. } => {}
        NegotiationResult::Reject { message, .. } => {
            panic!("Expected negotiations accepted, got: {}", message)
        }
        NegotiationResult::Negotiating { .. } => {
            panic!("Expected negotiations accepted, got: Negotiating")
        }
    }
}

pub fn expect_reject(result: NegotiationResult, error: Option<&str>) {
    match result {
        NegotiationResult::Ready { .. } => panic!("Expected negotiations rejected, got: Ready"),
        NegotiationResult::Negotiating { .. } => {
            panic!("Expected negotiations rejected, got: Negotiating")
        }
        NegotiationResult::Reject { message, is_final } => {
            assert!(is_final);
            if let Some(expected_error) = error {
                if !message.contains(expected_error) {
                    panic!(
                        "Negotiations error message: \n {} \n doesn't contain expected message: \n {}",
                        message, expected_error
                    );
                }
            }
        }
    }
}

pub mod cli {
    use assert_cmd::Command;
    use std::path::Path;

    pub fn list_rules_command(data_dir: &Path) -> serde_json::Value {
        let output = Command::cargo_bin("ya-provider")
            .unwrap()
            .env("DATA_DIR", data_dir.to_str().unwrap())
            .arg("rule")
            .arg("list")
            .arg("--json")
            .output()
            .unwrap();

        serde_json::from_slice(&output.stdout).unwrap()
    }

    pub fn list_certs(data_dir: &Path) -> Vec<String> {
        let output = Command::cargo_bin("ya-provider")
            .unwrap()
            .env("DATA_DIR", data_dir.to_str().unwrap())
            .arg("keystore")
            .arg("list")
            .arg("--json")
            .output()
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        result
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["ID"].as_str().unwrap().to_string())
            .collect()
    }

    pub fn rule_to_mode<'json>(
        rule: &'json serde_json::Value,
        cert_prefix: &str,
    ) -> Option<&'json serde_json::Value> {
        rule.as_object()
            .and_then(|obj| obj.iter().find(|(id, _cert)| id.starts_with(cert_prefix)))
            .map(|(_id, value)| &value["mode"])
    }

    pub fn remove_certificate_from_keystore(data_dir: &Path, cert_id: &str) {
        Command::cargo_bin("ya-provider")
            .unwrap()
            .env("DATA_DIR", data_dir.to_str().unwrap())
            .arg("keystore")
            .arg("remove")
            .arg(cert_id)
            .assert()
            .success();
    }

    pub fn add_certificate_to_keystore(
        data_dir: &Path,
        resource_cert_dir: &Path,
        cert: &str,
    ) -> String {
        Command::cargo_bin("ya-provider")
            .unwrap()
            .env("DATA_DIR", data_dir.to_str().unwrap())
            .arg("keystore")
            .arg("add")
            .arg(resource_cert_dir.join(cert))
            .assert()
            .success();

        list_certs(data_dir)[0].clone()
    }
}
