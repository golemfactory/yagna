mod utils;

use serial_test::serial;
use test_case::test_case;

use ya_provider::market::negotiator::builtin::allow_only::AllowOnly;
use ya_provider::market::negotiator::NegotiatorComponent;
use ya_provider::provider_agent::AgentNegotiatorsConfig;

use crate::utils::rules::{
    create_demand, create_offer, expect_accept, expect_reject, load_node_descriptor,
    setup_certificates_rules, setup_identity_rules, setup_rules_manager,
};

#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &["partner-certificate.signed.json"],
    &[];
    "Signed Requestors on the allow-list are passed"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &[],
    &[];
    "Signed Requestors not on the allow-list are passed"
)]
#[test_case(
    None,
    &["partner-certificate.signed.json"],
    &[];
    "Un-signed Requestors are passed"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &[],
    &["0x0000000000000000000000000000000000000000"];
    "Signed Requestors with identity on the allow-list are passed"
)]
#[test_case(
    None,
    &[],
    &["0x0000000000000000000000000000000000000001"];
    "Signed Requestors with identity not on the allow-list are passed"
)]
#[test_case(
    Some("node-descriptor-different-node.signed.json"),
    &[],
    &[];
    "Mismatching NodeId is ignored (passed)"
)]
#[test_case(
    Some("node-descriptor-invalid-signature.signed.json"),
    &[],
    &[];
    "Invalid signatures are ignored (passed)"
)]
#[serial]
fn allowonly_negotiator_rule_disabled(
    node_descriptor: Option<&str>,
    allow_certs: &[&str],
    allow_ids: &[&str],
) {
    let rules_manager = setup_rules_manager();
    rules_manager.allow_only().disable().unwrap();

    setup_certificates_rules(rules_manager.allow_only(), allow_certs);
    setup_identity_rules(rules_manager.allow_only(), allow_ids);

    let mut negotiator = AllowOnly::new(AgentNegotiatorsConfig { rules_manager });
    let demand = create_demand(load_node_descriptor(node_descriptor));

    let result = negotiator
        .negotiate_step(&demand, create_offer())
        .expect("Negotiator shouldn't return error");
    expect_accept(result);
}

#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &[],
    &[],
    "is not on the allow-only list";
    "Signed Requestors not on the allow-list are rejected"
)]
#[test_case(
    None,
    &[],
    &[],
    "is not on the allow-only list";
    "Un-Signed Requestors not on the allow-list are rejected"
)]
#[test_case(
    None,
    &["independent-chain-depth-1.cert.signed.json"],
    &[],
    "is not on the allow-only list";
    "Un-Signed Requestors rejected, when other certificate is allowed"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &["independent-chain-depth-1.cert.signed.json"],
    &[],
    "is not on the allow-only list";
    "Signed Requestors rejected, when other certificate is allowed"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &[],
    &["0x0000000000000000000000000000000000000001"],
    "is not on the allow-only list";
    "Signed Requestors rejected, when other identity is allowed"
)]
#[test_case(
    None,
    &[],
    &["0x0000000000000000000000000000000000000001"],
    "is not on the allow-only list";
    "UnSigned Requestors rejected, when other identity is allowed"
)]
#[test_case(
    Some("node-descriptor-happy-path.signed.json"),
    &["independent-chain-depth-1.cert.signed.json"],
    &["0x0000000000000000000000000000000000000001"],
    "is not on the allow-only list";
    "Signed Requestors rejected, when other identity and certificate is allowed"
)]
#[test_case(
    None,
    &["independent-chain-depth-1.cert.signed.json"],
    &["0x0000000000000000000000000000000000000001"],
    "is not on the allow-only list";
    "UnSigned Requestors rejected, when other identity and certificate is allowed"
)]
#[test_case(
    Some("node-descriptor-invalid-signature.signed.json"),
    &["partner-certificate.signed.json"],
    &[],
    "rejected due to suspicious behavior: AllowOnly rule: verification of node descriptor failed: Invalid signature value";
    "Signed Requestors rejected, when his signature is invalid"
)]
#[test_case(
    Some("node-descriptor-different-node.signed.json"),
    &["partner-certificate.signed.json"],
    &[],
    "rejected due to suspicious behavior: Node ids mismatch";
    "Signed Requestors rejected, when there is mismatch between certificate and Proposal NodeId"
)]
#[serial]
fn allowonly_negotiator_rule_rejections(
    node_descriptor: Option<&str>,
    allow_certs: &[&str],
    allow_ids: &[&str],
    expected_err: &str,
) {
    let rules_manager = setup_rules_manager();
    rules_manager.allow_only().enable().unwrap();

    setup_certificates_rules(rules_manager.allow_only(), allow_certs);
    setup_identity_rules(rules_manager.allow_only(), allow_ids);

    let mut negotiator = AllowOnly::new(AgentNegotiatorsConfig { rules_manager });
    let demand = create_demand(load_node_descriptor(node_descriptor));

    let result = negotiator
        .negotiate_step(&demand, create_offer())
        .expect("Negotiator shouldn't return error");
    expect_reject(result, Some(expected_err));
}
