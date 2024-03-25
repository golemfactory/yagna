mod utils;

use serial_test::serial;
use std::str::FromStr;
use test_case::test_case;

use ya_client_model::NodeId;
use ya_provider::market::negotiator::builtin::blacklist::Blacklist;
use ya_provider::market::negotiator::NegotiatorComponent;
use ya_provider::provider_agent::AgentNegotiatorsConfig;

use crate::utils::rules::{
    create_demand, create_offer, expect_accept, expect_reject, load_node_descriptor,
    setup_certificates_rules, setup_identity_rules, setup_rules_manager,
};

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
