mod utils;

use serial_test::serial;
use test_case::test_case;

use ya_provider::market::negotiator::builtin::allow_only::AllowOnly;
use ya_provider::market::negotiator::NegotiatorComponent;
use ya_provider::provider_agent::AgentNegotiatorsConfig;

use crate::utils::rules::{
    create_demand, create_offer, expect_accept, load_node_descriptor, setup_rules_manager,
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
fn allowonly_negotiator_rule_disabled(node_descriptor: Option<&str>) {
    let rules_manager = setup_rules_manager();
    rules_manager.allow_only().disable().unwrap();

    let mut negotiator = AllowOnly::new(AgentNegotiatorsConfig { rules_manager });
    let demand = create_demand(load_node_descriptor(node_descriptor));

    let result = negotiator
        .negotiate_step(&demand, create_offer())
        .expect("Negotiator shouldn't return error");
    expect_accept(result);
}
