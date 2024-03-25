use hex::ToHex;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
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

fn fingerprint(input_file_path: &Path) -> anyhow::Result<String> {
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
