#[macro_use]
extern crate serial_test;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use test_case::test_case;
use ya_agreement_utils::agreement::expand;
use ya_agreement_utils::{OfferTemplate, ProposalView};
use ya_client_model::market::proposal::State;
use ya_manifest_test_utils::{load_certificates_from_dir, TestResources};
use ya_manifest_utils::{Policy, PolicyConfig};
use ya_provider::market::negotiator::builtin::ManifestSignature;
use ya_provider::market::negotiator::*;
use ya_provider::provider_agent::AgentNegotiatorsConfig;
use ya_provider::rules::RulesManager;

static MANIFEST_TEST_RESOURCES: TestResources = TestResources {
    temp_dir: env!("CARGO_TARGET_TMPDIR"),
};

struct Signature<'a> {
    private_key_file: Option<&'a str>,
    algorithm: Option<&'a str>,
    certificate: Option<&'a str>,
}

#[test]
#[serial]
fn manifest_negotiator_test_accepted_because_outbound_is_not_requested() {
    // compManifest.net.inet.out.urls is empty, therefore outbound is not needed
    let urls = &[];

    let whitelist = r#"{ "patterns": [] }"#;
    let rulestore = r#"{"outbound": {"enabled": false, "everyone": "none"}}"#;

    let comp_manifest_b64 = create_comp_manifest_b64(urls);

    manifest_negotiator_test_encoded_manifest_without_signature(
        rulestore,
        whitelist,
        comp_manifest_b64,
        None,
    )
}

#[test]
#[serial]
fn manifest_negotiator_test_accepted_because_of_no_payload() {
    let payload = None;

    let rulestore = r#"{"outbound": {"enabled": false, "everyone": "none"}}"#;
    let whitelist = r#"{ "patterns": [] }"#;

    let (_, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    let whitelist_file = create_whitelist_file(whitelist);
    let rules_file_name = test_cert_dir.join("rules.json");
    let mut rules_file = std::fs::File::create(&rules_file_name).unwrap();
    rules_file.write_all(rulestore.as_bytes()).unwrap();

    let rules_manager =
        RulesManager::load_or_create(&rules_file_name, &whitelist_file, &test_cert_dir)
            .expect("Can't load RulesManager");

    let config = create_manifest_signature_validating_policy_config();
    let negotiator_cfg = AgentNegotiatorsConfig { rules_manager };
    let mut manifest_negotiator = ManifestSignature::new(&config, negotiator_cfg);
    // Current implementation does not verify content of certificate permissions incoming in demand.

    let demand = create_demand_json(payload);
    let demand = create_demand(demand);
    let offer = create_offer();

    let negotiation_result = manifest_negotiator.negotiate_step(&demand, offer.clone());
    let negotiation_result = negotiation_result.expect("Negotiator had not failed");

    assert_eq!(negotiation_result, NegotiationResult::Ready { offer });
}

#[test_case(
    r#"{"outbound": {"enabled": false, "everyone": "all"}}"#, // rulestore config
    &["https://domain.com"],
    Some("outbound is disabled"); // error msg
    "Rejected because outbound is disabled"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "all"}}"#, // rulestore config
    &["https://domain.com"],
    None; // error msg
    "Accepted because everyone is set to all"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none"}}"#, // rulestore config
    &["https://domain.com"],
    Some("Everyone rule is disabled"); // error msg
    "Rejected because everyone is set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist"}}"#, // rulestore config
    &["https://non-whitelisted.com"],
    Some("Everyone rule didn't match whitelist"); // error msg
    "Rejected because domain NOT whitelisted"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist"}}"#, // rulestore config
    &["https://domain.com"],
    None; // error msg
    "Accepted because everyone whitelist matched"
)]
#[serial]
fn manifest_negotiator_test_manifest_with_urls(
    rulestore: &str,
    urls: &[&str],
    error_msg: Option<&str>,
) {
    // compManifest.net.inet.out.urls is not empty, therefore outbound is required
    let whitelist = r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#;

    let comp_manifest_b64 = create_comp_manifest_b64(urls);

    manifest_negotiator_test_encoded_manifest_without_signature(
        rulestore,
        whitelist,
        comp_manifest_b64,
        error_msg,
    )
}

#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673": {"mode": "all", "description": ""}}}}"#, // rulestore config
    &["https://non-whitelisted.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because audited-payload all even if everyone-whitelist is mismatching"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673": {"mode": "whitelist", "description": ""}}}}"#, // rulestore config
    &["https://non-whitelisted.com"], // compManifest.net.inet.out.urls
    Some("Audited-Payload rule didn't match whitelist"); // error msg
    "Rejected because everyone and audited-payload whitelist are mismatching"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673": {"mode": "none", "description": ""}}}}"#, // rulestore config
    &["https://non-whitelisted.com"], // compManifest.net.inet.out.urls
    Some("Audited-Payload rule is disabled"); // error msg
    "Rejected because everyone and audited-payload set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673": {"mode": "all", "description": ""}}}}"#, // rulestore config
    &["https://non-whitelisted.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because audited-payload set to all"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673": {"mode": "whitelist", "description": ""}}}}"#, // rulestore config
    &["https://non-whitelisted.com"], // compManifest.net.inet.out.urls
    Some("Audited-Payload rule didn't match whitelist"); // error msg
    "Rejected because audited-payload whitelist doesn't match"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"55e451bd1a2f43570a25052b863af1d527fe6fd4bfd1482fdb241596432477f20eb2b2f3801fb5c6cd785f1a03c43ccf71fd8cdf0a974d1296be2326b0824673": {"mode": "whitelist", "description": ""}}}}"#, // rulestore config
    &["https://domain.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because domain is whitelisted when audited-payload set to whitelist"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "all"}}"#, // rulestore config
    &["https://domain.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because everyone is set to all even if audited-payload set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist"}}"#, // rulestore config
    &["https://domain.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because everyone whitelist is matching even if audited-payload set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist"}}"#, // rulestore config
    &["https://non-whitelisted.com"], // compManifest.net.inet.out.urls
    Some("Everyone rule didn't match whitelist ; Audited-Payload rule whole chain of cert_ids is not trusted"); // error msg
    "Rejected because everyone-whitelist is mismatching and audited-payload set to none"
)]
#[serial]
fn manifest_negotiator_test_with_valid_payload_signature(
    rulestore: &str,
    urls: &[&str],
    error_msg: Option<&str>,
) {
    // valid signature
    let signature = Signature {
        private_key_file: Some("foo_req.key.pem"),
        algorithm: Some("sha256"),
        certificate: Some("foo_req.cert.pem"),
    };
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature.private_key_file.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    let whitelist = r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#;

    manifest_negotiator_test_encoded_manifest_sign_and_cert(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature.algorithm,
        cert_b64,
        error_msg,
    )
}

#[test_case(
    r#""cb16a2ed213c1cf7e14faa7cf05743bc145b8555ec2eedb6b12ba0d31d17846d2ed4341b048f2e43b1ca5195a347bfeb0cd663c9e6002a4adb7cc7385112d3cc": { "mode": "all", "description": ""}"#,
    &["https://domain.com"], // compManifest.net.inet.out.urls
    "node-descriptor-invalid-signature.signed.json",
    Some("Partner verification of node descriptor failed: signature error"); // error msg
    "Rejected because descriptor is not valid"
)]
#[test_case(
    r#""cb16a2ed213c1cf7e14faa7cf05743bc145b8555ec2eedb6b12ba0d31d17846d2ed4341b048f2e43b1ca5195a347bfeb0cd663c9e6002a4adb7cc7385112d3cc": { "mode": "all", "description": ""}"#,
    &["https://domain.com"], // compManifest.net.inet.out.urls
    "node-descriptor-different-node.signed.json",
    Some("Partner rule nodes mismatch"); // error msg
    "Rejected because descriptor is meant for different node id"
)]
#[test_case(
    r#""cb16a2ed213c1cf7e14faa7cf05743bc145b8555ec2eedb6b12ba0d31d17846d2ed4341b048f2e43b1ca5195a347bfeb0cd663c9e6002a4adb7cc7385112d3cc": { "mode": "all", "description": ""}"#,
    &["https://domain.com"], // compManifest.net.inet.out.urls
    "node-descriptor-no-permissions.signed.json",
    Some("Partner No outbound permissions"); // error msg
    "Rejected because descriptor doesn't have any permissions"
)]
#[test_case(
    r#""cb16a2ed213c1cf7e14faa7cf05743bc145b8555ec2eedb6b12ba0d31d17846d2ed4341b048f2e43b1ca5195a347bfeb0cd663c9e6002a4adb7cc7385112d3cc": { "mode": "all", "description": ""}"#,
    &["https://different-domain.com"], // compManifest.net.inet.out.urls
    "node-descriptor-happy-path.signed.json",
    Some("Partner Partner rule forbidden url requested: https://different-domain.com/"); // error msg
    "Rejected because descriptor doesn't have url permissions"
)]
#[test_case(
    r#""different_trusted_id": { "mode": "all", "description": ""}"#,
    &["https://domain.com"], // compManifest.net.inet.out.urls
    "node-descriptor-happy-path.signed.json",
    Some("Partner rule whole chain of cert_ids is not trusted"); // error msg
    "Rejected because cert chain is not trusted"
)]
#[test_case(
    r#""cb16a2ed213c1cf7e14faa7cf05743bc145b8555ec2eedb6b12ba0d31d17846d2ed4341b048f2e43b1ca5195a347bfeb0cd663c9e6002a4adb7cc7385112d3cc": { "mode": "whitelist", "description": ""}"#,
    &["https://domain.com"], // compManifest.net.inet.out.urls
    "node-descriptor-happy-path.signed.json",
    None; // error msg
    "Accepted because valid descriptor is trusted to valid whitelist"
)]
#[test_case(
    r#""cb16a2ed213c1cf7e14faa7cf05743bc145b8555ec2eedb6b12ba0d31d17846d2ed4341b048f2e43b1ca5195a347bfeb0cd663c9e6002a4adb7cc7385112d3cc": { "mode": "all", "description": ""}"#,
    &["https://domain.com"], // compManifest.net.inet.out.urls
    "node-descriptor-happy-path.signed.json",
    None; // error msg
    "Accepted because valid descriptor is trusted to all"
)]
#[test_case(
    r#""cb16a2ed213c1cf7e14faa7cf05743bc145b8555ec2eedb6b12ba0d31d17846d2ed4341b048f2e43b1ca5195a347bfeb0cd663c9e6002a4adb7cc7385112d3cc": { "mode": "none", "description": ""}"#,
    &["https://domain.com"], // compManifest.net.inet.out.urls
    "node-descriptor-happy-path.signed.json",
    Some("Partner rule is disabled"); // error msg
    "Rejected because valid descriptor is not trusted"
)]
#[serial]
fn manifest_negotiator_test_with_node_identity(
    partner_rule: &str,
    urls: &[&str],
    descriptor_file: &str,
    error_msg: Option<&str>,
) {
    let rulestore = format!(
        r#"{{"outbound": {{"enabled": true, "everyone": "none", "audited-payload": {{"default": {{"mode": "none", "description": ""}}}}, "partner": {{ {} }}}}}}"#,
        partner_rule
    );

    let comp_manifest_b64 = create_comp_manifest_b64(urls);

    let whitelist = r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#;

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        &rulestore,
        whitelist,
        comp_manifest_b64,
        None,
        None,
        None,
        error_msg,
        &["partner-certificate.signed.json"],
        Some(descriptor_file),
    )
}

#[test]
#[serial]
fn manifest_negotiator_rejected_because_whitelist_doesnt_allow_unrestricted_access() {
    let rulestore = r#"{"outbound": {"enabled": true, "everyone": "whitelist"}}"#;
    let comp_manifest_b64 = create_comp_manifest_unrestricted_b64();
    let whitelist = r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#;

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        &rulestore,
        whitelist,
        comp_manifest_b64,
        None,
        None,
        None,
        Some("Everyone rule didn't match whitelist"),
        &[],
        None,
    )
}

#[test]
#[serial]
fn manifest_negotiator_with_node_identity_rejected_because_descriptor_doesnt_allow_unrestricted_access(
) {
    let partner_rule = r#""cb16a2ed213c1cf7e14faa7cf05743bc145b8555ec2eedb6b12ba0d31d17846d2ed4341b048f2e43b1ca5195a347bfeb0cd663c9e6002a4adb7cc7385112d3cc": { "mode": "all", "description": ""}"#;

    let rulestore = format!(
        r#"{{"outbound": {{"enabled": true, "everyone": "none", "audited-payload": {{"default": {{"mode": "none", "description": ""}}}}, "partner": {{ {} }}}}}}"#,
        partner_rule
    );
    let comp_manifest_b64 = create_comp_manifest_unrestricted_b64();
    let whitelist = r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#;

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        &rulestore,
        whitelist,
        comp_manifest_b64,
        None,
        None,
        None,
        Some("Partner Manifest tries to use Unrestricted access, but certificate allows only for specific urls"),
        &["partner-certificate.signed.json"],
        Some("node-descriptor-happy-path.signed.json"),
    )
}

#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "all"}}"#, // rulestore config
    &["https://domain.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because everyone is set to all"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist"}}"#, // rulestore config
    &["https://domain.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because everyone whitelist is matching"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist"}}"#, // rulestore config
    &["https://non-whitelisted.com"], // compManifest.net.inet.out.urls
    Some("Outbound rejected because: Everyone rule didn't match whitelist ; Audited-Payload rule: Invalid signature. ;"); // error msg
    "Rejected because everyone whitelist mismatched"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none"}}"#, // rulestore config
    &["https://domain.com"], // compManifest.net.inet.out.urls
    Some("Outbound rejected because: Everyone rule is disabled ; Audited-Payload rule: Invalid signature. ;"); // error msg
    "Rejected because everyone is set to none"
)]
#[serial]
fn manifest_negotiator_test_with_invalid_payload_signature(
    rulestore: &str,
    urls: &[&str],
    error_msg: Option<&str>,
) {
    // invalid signature
    let signature = Signature {
        private_key_file: Some("broken_signature"),
        algorithm: Some("sha256"),
        certificate: Some("foo_req.cert.pem"),
    };
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    let whitelist = r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#;

    manifest_negotiator_test_encoded_manifest_sign_and_cert(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature.private_key_file.map(|sig| sig.to_string()),
        signature.algorithm,
        cert_b64,
        error_msg,
    )
}

#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#, // data_dir/domain_whitelist.json
    &["https://domain.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because domain is whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#, // data_dir/domain_whitelist.json
    &["https://xdomain.com"], // compManifest.net.inet.out.urls
    Some("Everyone rule didn't match whitelist"); // error msg
    "Rejected because not exact match and match type is strict - leading characters"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#, // data_dir/domain_whitelist.json
    &["https://domain.comx"], // compManifest.net.inet.out.urls
    Some("Everyone rule didn't match whitelist"); // error msg
    "Rejected because not exact match and match type is strict - following characters"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "match": "strict" }] }"#, // data_dir/domain_whitelist.json
    &["https://x.domain.com"], // compManifest.net.inet.out.urls
    Some("Everyone rule didn't match whitelist"); // error msg
    "Rejected because not exact match and match type is strict - subdomain"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "a.com", "match": "strict" }, { "domain": "b.com", "match": "strict" }] }"#, // data_dir/domain_whitelist.json
    &["https://c.com"], // compManifest.net.inet.out.urls
    Some("Everyone rule didn't match whitelist"); // error msg
    "Rejected because domain not whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "a.com", "match": "strict" }, { "domain": "b.com", "match": "strict" }] }"#, // data_dir/domain_whitelist.json
    &["https://a.com", "https://c.com"], // compManifest.net.inet.out.urls
    Some("Everyone rule didn't match whitelist"); // error msg
    "Rejected because one of domains not whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "do.*ain.com", "match": "regex" }] }"#, // data_dir/domain_whitelist.json
    &["https://domain.com"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because domain is whitelisted (regex)"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "match": "regex" }] }"#, // data_dir/domain_whitelist.json
    &["https://domain.com.hacked.pro"], // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because domain is whitelisted (open ended regex - subdomain)"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "match": "regex" }] }"#, // data_dir/domain_whitelist.json
    &["https://mydomain.com"],
    None; // error msg
    "Accepted because domain is whitelisted (open ended regex - extended domain name)"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "^.*\\.domain.com$", "match": "regex" }] }"#, // data_dir/domain_whitelist.json
    &["https://valid.domain.com"],
    None; // error msg
    "Accepted because regex is allowing subdomains"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "^.*\\.domain.com$", "match": "regex" }] }"#, // data_dir/domain_whitelist.json
    &["https://mydomain.com"],
    Some("Everyone rule didn't match whitelist"); // error msg
    "Rejected because domain name does not match regex"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "^.*\\.domain.com$", "match": "regex" }] }"#, // data_dir/domain_whitelist.json
    &["https://domain.com.hacked.pro"],
    Some("Everyone rule didn't match whitelist"); // error msg
    "Rejected because regex does not allow different ending"
)]
#[serial]
fn manifest_negotiator_test_whitelist(whitelist: &str, urls: &[&str], error_msg: Option<&str>) {
    let rulestore = r#"{"outbound": {"enabled": true, "everyone": "whitelist"}}"#;

    // signature does not matter here
    let signature = Signature {
        private_key_file: None,
        algorithm: None,
        certificate: None,
    };
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature.private_key_file.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    manifest_negotiator_test_encoded_manifest_sign_and_cert(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature.algorithm,
        cert_b64,
        error_msg,
    )
}

fn manifest_negotiator_test_encoded_manifest_without_signature(
    rulestore: &str,
    whitelist: &str,
    comp_manifest_b64: String,
    error_msg: Option<&str>,
) {
    manifest_negotiator_test_encoded_manifest_sign_and_cert(
        rulestore,
        whitelist,
        comp_manifest_b64,
        None,
        None,
        None,
        error_msg,
    )
}

#[allow(clippy::too_many_arguments)]
fn manifest_negotiator_test_encoded_manifest_sign_and_cert(
    rulestore: &str,
    whitelist: &str,
    comp_manifest_b64: String,
    signature_b64: Option<String>,
    signature_alg: Option<&str>,
    cert_b64: Option<String>,
    error_msg: Option<&str>,
) {
    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature_alg,
        cert_b64,
        error_msg,
        &["foo_ca-chain.cert.pem"],
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
    rulestore: &str,
    whitelist: &str,
    comp_manifest_b64: String,
    signature_b64: Option<String>,
    signature_alg: Option<&str>,
    cert_b64: Option<String>,
    error_msg: Option<&str>,
    provider_certs: &[&str],
    node_descriptor_filename: Option<&str>,
) {
    // Having
    let (resource_cert_dir, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    load_certificates_from_dir(&resource_cert_dir, &test_cert_dir, provider_certs);

    let node_descriptor = node_descriptor_filename.map(|node_descriptor_filename| {
        let data = std::fs::read(resource_cert_dir.join(node_descriptor_filename)).unwrap();
        serde_json::from_slice(&data).unwrap()
    });
    let whitelist_file = create_whitelist_file(whitelist);
    let rules_file_name = test_cert_dir.join("rules.json");
    let mut rules_file = std::fs::File::create(&rules_file_name).unwrap();
    rules_file.write_all(rulestore.as_bytes()).unwrap();

    let rules_manager =
        RulesManager::load_or_create(&rules_file_name, &whitelist_file, &test_cert_dir)
            .expect("Can't load RulesManager");

    let config = create_manifest_signature_validating_policy_config();
    let negotiator_cfg = AgentNegotiatorsConfig { rules_manager };
    let mut manifest_negotiator = ManifestSignature::new(&config, negotiator_cfg);
    // Current implementation does not verify content of certificate permissions incoming in demand.

    let demand = create_demand_json(Some(Payload {
        comp_manifest_b64,
        signature_b64,
        signature_alg_b64: signature_alg,
        cert_b64,
        node_descriptor,
    }));
    let demand = create_demand(demand);
    let offer = create_offer();

    // When
    let negotiation_result = manifest_negotiator.negotiate_step(&demand, offer.clone());

    // Then
    let negotiation_result = negotiation_result.expect("Negotiator had not failed");
    if let Some(expected_error) = error_msg {
        match negotiation_result {
            NegotiationResult::Reject { message, is_final } => {
                assert!(is_final);
                if !message.contains(expected_error) {
                    panic!(
                        "Negotiations error message: \n {} \n doesn't contain expected message: \n {}",
                        message, expected_error
                    );
                }
            }
            _ => panic!("Expected negotiations rejected"),
        }
    } else {
        assert_eq!(negotiation_result, NegotiationResult::Ready { offer });
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

fn cert_file_to_cert_b64(cert_file: &str) -> String {
    let (resource_cert_dir, _) = MANIFEST_TEST_RESOURCES.init_cert_dirs();
    let mut cert_path = resource_cert_dir;
    cert_path.push(cert_file);
    println!("{cert_path:?}");
    let cert = fs::read(cert_path).expect("Can read cert from resources");
    base64::encode(cert)
}

fn create_comp_manifest_b64(urls: &[&str]) -> String {
    let manifest = json!({
        "version": "0.1.0",
        "createdAt": "2022-09-07T02:57:00.000000Z",
        "expiresAt": "2100-01-01T00:01:00.000000Z",
        "metadata": { "name": "App", "version": "0.1.0" },
        "payload": [],
        "compManifest": {
            "version": "0.1.0",
            "script": { "commands": [], "match": "regex" },
            "net": {
                "inet": {
                    "out": {
                        "protocols": ["https"],
                        "urls": urls
                    }
                }
            }
        }
    });
    base64::encode(serde_json::to_string(&manifest).unwrap())
}

fn create_comp_manifest_unrestricted_b64() -> String {
    let manifest = json!({
        "version": "0.1.0",
        "createdAt": "2022-09-07T02:57:00.000000Z",
        "expiresAt": "2100-01-01T00:01:00.000000Z",
        "metadata": { "name": "App", "version": "0.1.0" },
        "payload": [],
        "compManifest": {
            "version": "0.1.0",
            "script": { "commands": [], "match": "regex" },
            "net": {
                "inet": {
                    "out": {
                        "protocols": ["https"],
                        "unrestricted": {
                            "urls": true
                        }
                    }
                }
            }
        }
    });
    base64::encode(serde_json::to_string(&manifest).unwrap())
}

struct Payload<'a> {
    comp_manifest_b64: String,
    signature_b64: Option<String>,
    signature_alg_b64: Option<&'a str>,
    cert_b64: Option<String>,
    node_descriptor: Option<serde_json::Value>,
}

fn create_demand_json(payload: Option<Payload>) -> Value {
    let manifest = payload.map_or(
        json!({
            "golem": {
                "srv": {
                    "comp":{}
                }
            },
        }),
        |p| {
            let mut payload = HashMap::new();
            payload.insert("@tag", json!(p.comp_manifest_b64));
            if let (Some(sig), Some(alg)) = (&p.signature_b64, p.signature_alg_b64) {
                payload.insert(
                    "sig",
                    json!({
                        "@tag": sig.to_string(),
                        "algorithm": alg.to_string()
                    }),
                );
            } else if let Some(sig) = p.signature_b64 {
                payload.insert("sig", json!(sig));
            } else if let Some(alg) = p.signature_alg_b64 {
                payload.insert("sig", json!({ "algorithm": alg.to_string() }));
            }

            if let Some(cert) = &p.cert_b64 {
                payload.insert(
                    "cert",
                    json!({
                        "@tag": cert.to_string()
                    }),
                );
            } else if let Some(cert_b64) = p.cert_b64 {
                payload.insert("cert", json!(cert_b64));
            }

            json!({
                "golem": {
                    "srv": {
                        "comp": {
                            "payload": payload
                        }
                    },
                    "!exp": {
                        "gap-31": {
                            "v0": {
                                "node": {
                                    "descriptor": p.node_descriptor
                                }
                            }
                        }

                    }
                },
            })
        },
    );

    println!(
        "Tested demand:\n{}",
        serde_json::to_string_pretty(&manifest).unwrap()
    );

    manifest
}

fn create_manifest_signature_validating_policy_config() -> PolicyConfig {
    let mut config = PolicyConfig::default();
    config
        .policy_disable_component
        .push(Policy::ManifestCompliance);
    config
        .policy_disable_component
        .push(Policy::ManifestInetUrlCompliance);
    config
        .policy_disable_component
        .push(Policy::ManifestScriptCompliance);
    config
}

fn create_whitelist_file(whitelist_json: &str) -> PathBuf {
    let whitelist_file = whitelist_file();
    if whitelist_file.exists() {
        fs::remove_file(&whitelist_file).expect("Can delete whitelist file");
    }
    fs::write(whitelist_file.as_path(), whitelist_json).expect("Can write whitelist file");
    whitelist_file
}

fn whitelist_file() -> PathBuf {
    tmp_resource("whitelist.json")
}

fn tmp_resource(name: &str) -> PathBuf {
    let mut resource = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    resource.push(name);
    resource
}
