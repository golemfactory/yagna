use serde_json::{json, Value};
use serial_test::serial;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use test_case::test_case;

use hex::ToHex;
use ya_agreement_utils::agreement::expand;
use ya_agreement_utils::{OfferTemplate, ProposalView};
use ya_client_model::market::proposal::State;
use ya_client_model::NodeId;
use ya_manifest_test_utils::TestResources;
use ya_provider::market::negotiator::builtin::blacklist::Blacklist;
use ya_provider::market::negotiator::{NegotiationResult, NegotiatorComponent};
use ya_provider::provider_agent::AgentNegotiatorsConfig;
use ya_provider::rules::RulesManager;

static MANIFEST_TEST_RESOURCES: TestResources = TestResources {
    temp_dir: env!("CARGO_TARGET_TMPDIR"),
};

fn setup_rules_manager() -> RulesManager {
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
            "root-cert-independent-chain.cert.signed.json",
            "independent-chain-depth-1.cert.signed.json",
            "independent-chain-depth-2.cert.signed.json",
            "independent-chain-depth-3.cert.signed.json",
        ],
    );

    rules_manager
}

fn import_certificates(rules: &mut RulesManager, certs: &[&str]) {
    let (resource_cert_dir, _test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    for cert in certs {
        let cert_path = resource_cert_dir.join(cert);
        rules.import_certs(&cert_path).unwrap();
    }
}

fn create_demand(demand: Value) -> ProposalView {
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

fn create_offer() -> ProposalView {
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

fn load_node_descriptor(file: Option<&str>) -> Value {
    let (resource_cert_dir, _test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    let desc = file
        .map(|node_descriptor_filename| {
            let data = std::fs::read(resource_cert_dir.join(node_descriptor_filename)).unwrap();
            serde_json::from_slice::<Value>(&data).unwrap()
        })
        .unwrap_or(Value::Null);

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
}

fn fingerprint(input_file_path: &Path) -> anyhow::Result<String> {
    let json_string = fs::read_to_string(input_file_path)?;
    let input_json: Value = serde_json::from_str(&json_string)?;

    let signed_data = &input_json["certificate"];
    let fingerprint = golem_certificate::create_default_hash(signed_data)?;
    Ok(fingerprint.encode_hex::<String>())
}

fn setup_certificates_rules(rules: &mut RulesManager, certs: &[&str]) {
    let (resource_cert_dir, _test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();
    for cert in certs {
        let path = resource_cert_dir.join(cert);
        let fingerprint = fingerprint(&path).unwrap();
        rules.blacklist().add_certified_rule(&fingerprint).unwrap();
    }
}

fn setup_identity_rules(rules: &mut RulesManager, ids: &[&str]) {
    for id in ids {
        rules
            .blacklist()
            .add_identity_rule(NodeId::from_str(id).unwrap())
            .unwrap();
    }
}

fn expect_accept(result: NegotiationResult) {
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

fn expect_reject(result: NegotiationResult, error: Option<&str>) {
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

#[test_case(
    Some("node-descriptor-happy-path.signed.json");
    "Signed Requestors are passed"
)]
#[test_case(None; "Un-signed Requestors are passed")]
#[test_case(
    Some("node-descriptor-different-node.signed.json");
    "Mismatching NodeId is ignored (passed)"
)]
#[test_case(
    Some("node-descriptor-invalid-signature.signed.json");
    "Invalid signatures are ignored (passed)"
)]
#[serial]
fn blacklist_negotiator_rule_disabled(node_descriptor: Option<&str>) {
    let rules_manager = setup_rules_manager();
    rules_manager.blacklist().disable().unwrap();

    let mut negotiator = Blacklist::new(AgentNegotiatorsConfig { rules_manager });
    let demand = create_demand(load_node_descriptor(node_descriptor));

    let result = negotiator
        .negotiate_step(&demand, create_offer())
        .expect("Negotiator shouldn't return error");
    expect_accept(result);
}

#[test_case(
    None,
    "Requestor's NodeId is on the blacklist";
    "Rejected because requestor is blacklisted"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    "Requestor's NodeId is on the blacklist";
    "Rejected because requestor is blacklisted and signature is ignored"
)]
#[test_case(
    Some("node-descriptor-invalid-signature.signed.json"),
    "Requestor's NodeId is on the blacklist";
    "Rejected because requestor is blacklisted and invalid signature is ignored"
)]
#[test_case(
    Some("node-descriptor-different-node.signed.json"),
    "Requestor's NodeId is on the blacklist";
    "Rejected because requestor is blacklisted and mismatching NodeId is ignored"
)]
#[serial]
fn blacklist_negotiator_id_blacklisted(node_descriptor: Option<&str>, expected_err: &str) {
    let rules_manager = setup_rules_manager();
    rules_manager.blacklist().enable().unwrap();
    rules_manager
        .blacklist()
        .add_identity_rule(NodeId::from_str("0x0000000000000000000000000000000000000000").unwrap())
        .unwrap();

    let mut negotiator = Blacklist::new(AgentNegotiatorsConfig { rules_manager });
    let demand = create_demand(load_node_descriptor(node_descriptor));

    let result = negotiator
        .negotiate_step(&demand, create_offer())
        .expect("Negotiator shouldn't return error");
    expect_reject(result, Some(expected_err));
}

#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &["partner-certificate.signed.json"],
    "Requestor's certificate is on the blacklist";
    "Rejected because certificate is on the blacklist"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &["partner-certificate.signed.json"],
    "Requestor's certificate is on the blacklist";
    "Rejected because top level certificate is on the blacklist"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &["partner-certificate.signed.json", "root-cert-independent-chain.cert.signed.json"],
    "Requestor's certificate is on the blacklist";
    "Sanity check with additional independent certificate"
)]
#[test_case(
    Some("node-descriptor-invalid-signature.signed.json"),
    &["partner-certificate.signed.json"],
    "rejected due to suspicious behavior: Blacklist rule: verification of node descriptor failed: Invalid signature value";
    "Rejected because Requestor has invalid signature"
)]
#[test_case(
    Some("node-descriptor-invalid-signature.signed.json"),
    &[],
    "rejected due to suspicious behavior: Blacklist rule: verification of node descriptor failed: Invalid signature value";
    "Rejected because Requestor has invalid signature, despite certificate is not on the blacklist"
)]
#[test_case(
    Some("node-descriptor-different-node.signed.json"),
    &["partner-certificate.signed.json"],
    "rejected due to suspicious behavior: Node ids mismatch";
    "Rejected because of NodeId mismatch in Proposal and in signature"
)]
#[test_case(
    Some("node-descriptor-different-node.signed.json"),
    &[],
    "rejected due to suspicious behavior: Node ids mismatch";
    "Rejected because of NodeId mismatch in Proposal and in signature, despite certificate is not on the blacklist"
)]
#[serial]
fn blacklist_negotiator_certificate_blacklisted(
    node_descriptor: Option<&str>,
    blacklist_certs: &[&str],
    expected_err: &str,
) {
    let mut rules_manager = setup_rules_manager();

    rules_manager.blacklist().enable().unwrap();
    setup_certificates_rules(&mut rules_manager, blacklist_certs);

    let mut negotiator = Blacklist::new(AgentNegotiatorsConfig { rules_manager });
    let demand = create_demand(load_node_descriptor(node_descriptor));

    let result = negotiator
        .negotiate_step(&demand, create_offer())
        .expect("Negotiator shouldn't return error");
    expect_reject(result, Some(expected_err));
}

#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &["root-cert-independent-chain.cert.signed.json"],
    &["0x0000000000000000000000000000000000000001"];
    "Accepted; Other certificate and id is on blacklist"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &["root-cert-independent-chain.cert.signed.json"],
    &[];
    "Accepted; Other certificate is on blacklist"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &[],
    &["0x0000000000000000000000000000000000000001"];
    "Accepted; Other id is on blacklist"
)]
#[serial]
fn blacklist_negotiator_pass_node(
    node_descriptor: Option<&str>,
    blacklist_certs: &[&str],
    blacklist_ids: &[&str],
) {
    let mut rules_manager = setup_rules_manager();
    rules_manager.blacklist().enable().unwrap();

    setup_certificates_rules(&mut rules_manager, blacklist_certs);
    setup_identity_rules(&mut rules_manager, blacklist_ids);

    let mut negotiator = Blacklist::new(AgentNegotiatorsConfig { rules_manager });
    let demand = create_demand(load_node_descriptor(node_descriptor));

    let result = negotiator
        .negotiate_step(&demand, create_offer())
        .expect("Negotiator shouldn't return error");
    expect_accept(result);
}
