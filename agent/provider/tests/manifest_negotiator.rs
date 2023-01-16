#[macro_use]
extern crate serial_test;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use test_case::test_case;
use ya_agreement_utils::AgreementView;
use ya_manifest_test_utils::{load_certificates_from_dir, TestResources};
use ya_manifest_utils::policy::CertPermissions;
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
    signature: Option<&'a str>,
    certificate: Option<&'a str>,
}

//below to be redesigned
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "all", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "do.*ain.com", "type": "regex" }, { "domain": "another.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Signature { private_key_file: None, signature: None, certificate: None},
    None; // error msg
    "Manifest without signature accepted because domain whitelisted (regex pattern)"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }, { "domain": "another.whitelisted.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com", "https://not.whitelisted.com"]"#, // compManifest.net.inet.out.urls
    Signature { private_key_file: None, signature: None, certificate: None},
    Some("Didn't match any Rules"); // error msg
    "Manifest without signature rejected because ONE of domains NOT whitelisted"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Signature { private_key_file: Some("foo_req.key.pem"), signature: Some("sha256"), certificate: Some("dummy_inter.cert.pem") }, // signature with untrusted cert
    Some("failed to verify manifest signature: Invalid certificate"); // error msg
    "Manifest rejected because of invalid certificate even when urls domains are whitelisted"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Signature { private_key_file: Some("foo_inter.key.pem"), signature: Some("sha256"), certificate: Some("foo_req.cert.pem")}, // signature with private key file not matching cert
    Some("failed to verify manifest signature: Invalid signature"); // error msg
    "Manifest rejected because of invalid signature (signed using incorrect private key) even when urls domains are whitelisted"
)]
#[serial]
fn manifest_negotiator_test(
    rulestore: &str,
    whitelist: &str,
    urls: &str,
    signature: Signature,
    error_msg: Option<&str>,
) {
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature.private_key_file.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature.signature,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
        &["foo_ca-chain.cert.pem"],
    )
}

#[test_case(
    r#"{"outbound": {"enabled": false, "everyone": "none", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [] }"#, // data_dir/domain_whitelist.json
    None; // error msg
    "Manifest accepted because its urls list is empty"
)]
#[serial]
fn manifest_negotiator_test_manifest_without_urls(
    rulestore: &str,
    whitelist: &str,
    error_msg: Option<&str>,
) {
    // compManifest.net.inet.out.urls is empty, therefore outbound is not needed
    let urls = "[]";

    // signature does not matter here
    let signature = Signature {
        private_key_file: None,
        signature: None,
        certificate: None,
    };
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature.private_key_file.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature.signature,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
        &["foo_ca-chain.cert.pem"],
    )
}

#[test_case(
    r#"{"outbound": {"enabled": false, "everyone": "all", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"{"patterns": [] }"#, // data_dir/domain_whitelist.json
    Some("outbound is disabled"); // error msg
    "Manifest with outbound is not accepted because outbound is disabled"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "all", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [] }"#, // data_dir/domain_whitelist.json
    None; // error msg
    "Manifest without signature accepted because everyone is set to all"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "different_domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    Some("Didn't match any Rules"); // error msg
    "Manifest rejected because domain NOT whitelisted"
)]
#[serial]
fn manifest_negotiator_test_manifest_with_urls(
    rulestore: &str,
    whitelist: &str,
    error_msg: Option<&str>,
) {
    // compManifest.net.inet.out.urls is not empty, therefore outbound is required
    let urls = r#"["https://domain.com"]"#;

    // signature does not matter here
    let signature = Signature {
        private_key_file: None,
        signature: None,
        certificate: None,
    };
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature.private_key_file.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature.signature,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
        &["foo_ca-chain.cert.pem"],
    )
}

#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "all", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because everyone is set to all even if audited-payload set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because everyone whitelist is matching even if audited-payload set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://non-whitelisted.com"]"#, // compManifest.net.inet.out.urls
    Some("Audited-Payload rule is disabled"); // error msg
    "Rejected because everyone-whitelist is mismatching and audited-payload set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "all", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://non-whitelisted.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because audited-payload all even if everyone-whitelist is mismatching"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://non-whitelisted.com"]"#, // compManifest.net.inet.out.urls
    Some("Audited-Payload whitelist doesn't match"); // error msg
    "Rejected because everyone and audited-payload whitelist are mismatching"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://non-whitelisted.com"]"#, // compManifest.net.inet.out.urls
    Some("Audited-Payload rule is disabled"); // error msg
    "Rejected because everyone and audited-payload set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "all", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://non-whitelisted.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because audited-payload set to all"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://non-whitelisted.com"]"#, // compManifest.net.inet.out.urls
    Some("Audited-Payload whitelist doesn't match"); // error msg
    "Rejected because audited-payload whitelist doesn't match"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because domain is whitelisted when audited-payload set to whitelist"
)]
#[serial]
fn manifest_negotiator_test_with_valid_payload_signature(
    rulestore: &str,
    urls: &str,
    error_msg: Option<&str>,
) {
    // valid signature
    let signature = Signature {
        private_key_file: Some("foo_req.key.pem"),
        signature: Some("sha256"),
        certificate: Some("foo_req.cert.pem"),
    };
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature.private_key_file.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    let whitelist = r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }] }"#;

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature.signature,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
        &["foo_ca-chain.cert.pem"],
    )
}

#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "all", "audited-payload": {"default": {"mode": "all", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because everyone is set to all"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "all", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because everyone whitelist is matching"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "all", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://non-whitelisted.com"]"#, // compManifest.net.inet.out.urls
    Some("failed to verify manifest signature: Invalid signature"); // error msg
    "Rejected because everyone whitelist mismatched"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "all", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Some("failed to verify manifest signature: Invalid signature"); // error msg
    "Rejected because everyone is set to none"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Some("failed to verify manifest signature: Invalid signature"); // error msg
    "Rejected because everyone is not set to all even if audited-payload whitelist is matching"
)]
#[serial]
fn manifest_negotiator_test_with_invalid_payload_signature(
    rulestore: &str,
    urls: &str,
    error_msg: Option<&str>,
) {
    // invalid signature
    let signature = Signature {
        private_key_file: Some("broken_signature"),
        signature: Some("sha256"),
        certificate: Some("foo_req.cert.pem"),
    };
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    let whitelist = r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }] }"#;

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature.private_key_file.map(|sig| sig.to_string()),
        signature.signature,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
        &["foo_ca-chain.cert.pem"],
    )
}

#[test_case(
    r#"{"outbound": {"enabled": false, "everyone": "none", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [] }"#, // data_dir/domain_whitelist.json
    None; // error msg
    "Manifest accepted because no payload"
)]
#[serial]
fn manifest_negotiator_test_no_payload(rulestore: &str, whitelist: &str, error_msg: Option<&str>) {
    // Having
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

    let demand = create_demand_json(None);
    let demand = create_demand(demand);
    let offer = create_offer();

    // When
    let negotiation_result = manifest_negotiator.negotiate_step(&demand, offer.clone());

    // Then
    let negotiation_result = negotiation_result.expect("Negotiator had not failed");
    if let Some(message) = error_msg {
        assert_eq!(
            negotiation_result,
            NegotiationResult::Reject {
                message: message.to_string(),
                is_final: true
            }
        );
    } else {
        assert_eq!(negotiation_result, NegotiationResult::Ready { offer });
    }
}

#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because domain is whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "do.*ain.com", "type": "regex" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None; // error msg
    "Accepted because domain is whitelisted (regex)"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "a.com", "type": "strict" }, { "domain": "b.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://c.com"]"#, // compManifest.net.inet.out.urls
    Some("Didn't match any Rules"); // error msg
    "Rejected because domain not whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "a.com", "type": "strict" }, { "domain": "b.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://a.com", "https://c.com"]"#, // compManifest.net.inet.out.urls
    Some("Didn't match any Rules"); // error msg
    "Rejected because one of domains not whitelisted"
)]
#[serial]
fn manifest_negotiator_test_whitelist(whitelist: &str, urls: &str, error_msg: Option<&str>) {
    let rulestore = r#"{"outbound": {"enabled": true, "everyone": "whitelist", "audited-payload": {"default": {"mode": "none", "description": "default setting"}}}}"#;

    // signature does not matter here
    let signature = Signature {
        private_key_file: None,
        signature: None,
        certificate: None,
    };
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature.private_key_file.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });
    let cert_b64 = signature.certificate.map(cert_file_to_cert_b64);

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature.signature,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
        &["foo_ca-chain.cert.pem"],
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
    cert_permissions_b64: Option<&str>,
    error_msg: Option<&str>,
    provider_certs_permissions: &Vec<CertPermissions>,
    provider_certs: &[&str],
) {
    // Having
    let (resource_cert_dir, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    if signature_b64.is_some() {
        load_certificates_from_dir(
            &resource_cert_dir,
            &test_cert_dir,
            provider_certs,
            provider_certs_permissions,
        );
    }

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
        cert_permissions_b64,
    }));
    let demand = create_demand(demand);
    let offer = create_offer();

    // When
    let negotiation_result = manifest_negotiator.negotiate_step(&demand, offer.clone());

    // Then
    let negotiation_result = negotiation_result.expect("Negotiator had not failed");
    if let Some(message) = error_msg {
        assert_eq!(
            negotiation_result,
            NegotiationResult::Reject {
                message: message.to_string(),
                is_final: true
            }
        );
    } else {
        assert_eq!(negotiation_result, NegotiationResult::Ready { offer });
    }
}
fn create_demand(demand: Value) -> AgreementView {
    AgreementView {
        json: demand,
        agreement_id: "id".to_string(),
    }
}

fn create_offer() -> AgreementView {
    AgreementView {
        json: serde_json::from_str(r#"{ "any": "thing" }"#).unwrap(),
        agreement_id: "id".to_string(),
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

fn create_comp_manifest_b64(urls: &str) -> String {
    let manifest_template = r#"{
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
                        "urls": __URLS__
                    }
                }
            }
        }
    }"#;
    let manifest = manifest_template.replace("__URLS__", urls);
    base64::encode(manifest)
}

struct Payload<'a> {
    comp_manifest_b64: String,
    signature_b64: Option<String>,
    signature_alg_b64: Option<&'a str>,
    cert_b64: Option<String>,
    cert_permissions_b64: Option<&'a str>,
}

// fn create_demand_json(
//     comp_manifest_b64: &str,
//     signature_b64: Option<String>,
//     signature_alg_b64: Option<&str>,
//     cert_b64: Option<String>,
//     cert_permissions_b64: Option<&str>,
// ) -> serde_json::Value {
//     let mut payload = HashMap::new();
//     payload.insert("@tag", json!(comp_manifest_b64));
//     if signature_b64.is_some() && signature_alg_b64.is_some() {
//         payload.insert(
//             "sig",
//             json!({
//                 "@tag": signature_b64.unwrap(),
//                 "algorithm": signature_alg_b64.unwrap().to_string()
//             }),
//         );
//     } else if signature_b64.is_some() {
//         payload.insert("sig", json!(signature_b64.unwrap()));
//     } else if signature_alg_b64.is_some() {
//         payload.insert(
//             "sig",
//             json!({ "algorithm": signature_alg_b64.unwrap().to_string() }),
//         );
//     }
//
//     if cert_b64.is_some() && cert_permissions_b64.is_some() {
//         payload.insert(
//             "cert",
//             json!({
//                 "@tag": cert_b64.unwrap(),
//                 "permissions": cert_permissions_b64.unwrap().to_string()
//             }),
//         );
//     } else if let Some(cert_b64) = cert_b64 {
//         payload.insert("cert", json!(cert_b64));
//     }
//
//     // let mut payload = manifest.to_string();
//     let manifest = json!({
//         "golem": {
//             "srv": {
//                 "comp": {
//                     "payload": payload
//                 }
//             }
//         },
//     });
//     println!(
//         "Tested demand:\n{}",
//         serde_json::to_string_pretty(&manifest).unwrap()
//     );
//     manifest
// }

fn create_demand_json(payload: Option<Payload>) -> Value {
    match payload {
        Some(p) => {
            let mut payload = HashMap::new();
            payload.insert("@tag", json!(p.comp_manifest_b64));
            if p.signature_b64.is_some() && p.signature_alg_b64.is_some() {
                payload.insert(
                    "sig",
                    json!({
                        "@tag": p.signature_b64.unwrap(),
                        "algorithm": p.signature_alg_b64.unwrap().to_string()
                    }),
                );
            } else if p.signature_b64.is_some() {
                payload.insert("sig", json!(p.signature_b64.unwrap()));
            } else if p.signature_alg_b64.is_some() {
                payload.insert(
                    "sig",
                    json!({ "algorithm": p.signature_alg_b64.unwrap().to_string() }),
                );
            }

            if p.cert_b64.is_some() && p.cert_permissions_b64.is_some() {
                payload.insert(
                    "cert",
                    json!({
                        "@tag": p.cert_b64.unwrap(),
                        "permissions": p.cert_permissions_b64.unwrap().to_string()
                    }),
                );
            } else if let Some(cert_b64) = p.cert_b64 {
                payload.insert("cert", json!(cert_b64));
            }

            let manifest = json!({
                "golem": {
                    "srv": {
                        "comp": {
                            "payload": payload
                        }
                    }
                },
            });
            println!(
                "Tested demand:\n{}",
                serde_json::to_string_pretty(&manifest).unwrap()
            );
            manifest
        }
        _ => {
            let manifest = json!({
                "golem": {
                    "srv": {
                        "comp":{}
                    }
                },
            });
            println!(
                "Tested demand:\n{}",
                serde_json::to_string_pretty(&manifest).unwrap()
            );
            manifest
        }
    }
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
