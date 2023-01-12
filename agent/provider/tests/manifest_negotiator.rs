#![allow(clippy::too_many_arguments)]

#[macro_use]
extern crate serial_test;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use serde_json::json;
use test_case::test_case;
use ya_agreement_utils::AgreementView;
use ya_manifest_test_utils::{load_certificates_from_dir, TestResources};
use ya_manifest_utils::matching::domain::{DomainPatterns, DomainWhitelistState};
use ya_manifest_utils::policy::CertPermissions;
use ya_manifest_utils::{Keystore, Policy, PolicyConfig};
use ya_provider::market::negotiator::builtin::ManifestSignature;
use ya_provider::market::negotiator::*;
use ya_provider::provider_agent::AgentNegotiatorsConfig;
use ya_provider::rules::RulesManager;

static MANIFEST_TEST_RESOURCES: TestResources = TestResources {
    temp_dir: env!("CARGO_TARGET_TMPDIR"),
};

#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None, // private key file
    None, // sig alg
    None, // cert
    None; // error msg
    "Manifest without signature accepted because domain whitelisted"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "do.*ain.com", "type": "regex" }, { "domain": "another.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None, // private key file
    None, // sig alg
    None, // cert
    None; // error msg
    "Manifest without signature accepted because domain whitelisted (regex pattern)"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "different_domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    None, // private key file
    None, // sig alg
    None, // cert
    Some("manifest requires signature but it has none"); // error msg
    "Manifest without signature rejected because domain NOT whitelisted"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }, { "domain": "another.whitelisted.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com", "https://not.whitelisted.com"]"#, // compManifest.net.inet.out.urls
    None, // private key file
    None, // sig alg
    None, // cert
    Some("manifest requires signature but it has none"); // error msg
    "Manifest without signature rejected because ONE of domains NOT whitelisted"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }] }"#, // data_dir/domain_whitelist.json
    r#"[]"#, // compManifest.net.inet.out.urls
    None, // private key file
    None, // sig alg
    None, // cert
    None; // error msg
    "Manifest accepted because its urls list is empty"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    None; // error msg
    "Manifest accepted with url NOT whitelisted because signature valid"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("dummy_inter.cert.pem"), // untrusted cert
    Some("failed to verify manifest signature: Invalid certificate"); // error msg
    "Manifest rejected because of invalid certificate even when urls domains are whitelisted"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Some("foo_inter.key.pem"), // private key file not matching certificate
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // certificate not matching private key
    Some("failed to verify manifest signature: Invalid signature"); // error msg
    "Manifest rejected because of invalid signature (signed using incorrect private key) even when urls domains are whitelisted"
)]
#[serial]
fn manifest_negotiator_test(
    rulestore: &str,
    whitelist: &str,
    urls: &str,
    signing_key: Option<&str>,
    signature_alg: Option<&str>,
    cert: Option<&str>,
    error_msg: Option<&str>,
) {
    let comp_manifest_b64 = create_comp_manifest_b64(urls);

    let signature_b64 = signing_key.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });

    let cert_b64 = cert.map(cert_file_to_cert_b64);

    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature_alg,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
        &["foo_ca-chain.cert.pem"],
    )
}

#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    Some("broken_signature"), // signature (broken)
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    Some("failed to verify manifest signature: Invalid signature"); // error msg
    "Manifest rejected because of invalid signature"
)]
#[serial]
fn manifest_negotiator_test_encoded_sign_and_cert(
    rulestore: &str,
    whitelist: &str,
    urls: &str,
    signature_b64: Option<&str>,
    signature_alg: Option<&str>,
    cert: Option<&str>,
    error_msg: Option<&str>,
) {
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature_b64.map(|signature| signature.to_string());

    let cert_b64 = cert.map(cert_file_to_cert_b64);
    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        whitelist,
        comp_manifest_b64,
        signature_b64,
        signature_alg,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
        &["foo_ca-chain.cert.pem"],
    )
}

#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    None, // cert_permissions_b64
    &vec![CertPermissions::OutboundManifest],
    None;
    "Manifest accepted, because permissions are sufficient"
)]
#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    None, // cert_permissions_b64
    &vec![CertPermissions::All],
    None;
    "Manifest accepted, when permissions are set to `All`"
)]
#[serial]
fn test_manifest_negotiator_certs_permissions(
    rulestore: &str,
    signing_key: Option<&str>,
    signature_alg: Option<&str>,
    cert: Option<&str>,
    cert_permissions_b64: Option<&str>,
    provider_certs_permissions: &Vec<CertPermissions>,
    error_msg: Option<&str>,
) {
    manifest_negotiator_test_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        signing_key,
        signature_alg,
        cert,
        cert_permissions_b64,
        provider_certs_permissions,
        &["foo_ca-chain.cert.pem"],
        error_msg,
    )
}

#[test_case(
    r#"{"outbound": {"enabled": true, "everyone": "none", "audited-payload": {"default": {"mode": "whitelist", "description": "default setting"}}}}"#, // rulestore config
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_inter_req-chain.cert.pem"), // cert
    Some("NYI"), // cert_permissions_b64
    &vec![CertPermissions::All, CertPermissions::UnverifiedPermissionsChain],
    &["foo_ca.cert.pem"], // cert dir files
    None; // error msg
    "Certificate chain in Demand supported"
)]
#[serial]
fn manifest_negotiator_test_manifest_sign_and_cert_and_cert_dir_files(
    rulestore: &str,
    signing_key: Option<&str>,
    signature_alg: Option<&str>,
    cert: Option<&str>,
    cert_permissions_b64: Option<&str>,
    provider_certs_permissions: &Vec<CertPermissions>,
    provider_certs: &[&str],
    error_msg: Option<&str>,
) {
    let comp_manifest_b64 = create_comp_manifest_b64(r#"["https://domain.com"]"#);

    let signature_b64 = signing_key.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });

    let cert_b64 = cert.map(cert_file_to_cert_b64);
    manifest_negotiator_test_encoded_manifest_sign_and_cert_and_cert_dir_files(
        rulestore,
        r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#,
        comp_manifest_b64,
        signature_b64,
        signature_alg,
        cert_b64,
        cert_permissions_b64,
        error_msg,
        provider_certs_permissions,
        provider_certs,
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
    let whitelist_state = create_whitelist(whitelist);
    let (resource_cert_dir, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    if signature_b64.is_some() {
        load_certificates_from_dir(
            &resource_cert_dir,
            &test_cert_dir,
            provider_certs,
            provider_certs_permissions,
        );
    }

    let keystore = Keystore::load(&test_cert_dir).expect("Can load test certificates");

    let name = test_cert_dir.join("rules.json");
    let mut rules_file = std::fs::File::create(&name).unwrap();
    rules_file.write_all(rulestore.as_bytes()).unwrap();

    let rulestore = RulesManager::load_or_create(&name).expect("Can't load RuleStore");

    let config = create_manifest_signature_validating_policy_config();
    let negotiator_cfg = AgentNegotiatorsConfig {
        trusted_keys: keystore,
        domain_patterns: whitelist_state,
        rules_config: rulestore,
    };
    let mut manifest_negotiator = ManifestSignature::new(&config, negotiator_cfg);
    // Current implementation does not verify content of certificate permissions incoming in demand.

    let demand = create_demand_json(
        &comp_manifest_b64,
        signature_b64,
        signature_alg,
        cert_b64,
        cert_permissions_b64,
    );
    let demand = AgreementView {
        json: demand,
        agreement_id: "id".to_string(),
    };
    let offer = AgreementView {
        json: serde_json::from_str(r#"{ "any": "thing" }"#).unwrap(),
        agreement_id: "id".to_string(),
    };

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

#[test]
#[serial]
fn offer_should_be_rejected_when_outbound_is_disabled() {
    let (_, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    let rules_file = test_cert_dir.join("rules.json");
    let rules_config = RulesManager::load_or_create(&rules_file).unwrap();
    rules_config.set_enabled(false).unwrap();

    let config = create_manifest_signature_validating_policy_config();
    let negotiator_cfg = AgentNegotiatorsConfig {
        trusted_keys: Keystore::load(&test_cert_dir).expect("Can load test certificates"),
        domain_patterns: create_whitelist(
            r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#,
        ),
        rules_config,
    };
    let mut manifest_negotiator = ManifestSignature::new(&config, negotiator_cfg);

    let demand = AgreementView {
        json: create_demand_json(
            &create_comp_manifest_b64(r#"["https://domain.com"]"#),
            None,
            None,
            None,
            None,
        ),
        agreement_id: "id".into(),
    };
    let offer = AgreementView {
        json: serde_json::from_str(r#"{ "any": "thing" }"#).unwrap(),
        agreement_id: "id".into(),
    };

    let result = manifest_negotiator.negotiate_step(&demand, offer).unwrap();

    assert_eq!(
        result,
        NegotiationResult::Reject {
            message: "outbound is disabled".into(),
            is_final: true
        }
    );
}

#[test]
#[serial]
fn offer_should_be_accepted_when_url_list_is_empty() {
    let (_, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    let rules_file = test_cert_dir.join("rules.json");
    let rules_config = RulesManager::load_or_create(&rules_file).unwrap();
    rules_config.set_enabled(false).unwrap();

    let config = create_manifest_signature_validating_policy_config();
    let negotiator_cfg = AgentNegotiatorsConfig {
        trusted_keys: Keystore::load(&test_cert_dir).expect("Can load test certificates"),
        domain_patterns: create_whitelist(
            r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#,
        ),
        rules_config,
    };
    let mut manifest_negotiator = ManifestSignature::new(&config, negotiator_cfg);

    let demand = AgreementView {
        json: create_demand_json(&create_comp_manifest_b64(r#"[]"#), None, None, None, None),
        agreement_id: "id".into(),
    };
    let offer = AgreementView {
        json: serde_json::from_str(r#"{ "any": "thing" }"#).unwrap(),
        agreement_id: "id".into(),
    };

    let result = manifest_negotiator
        .negotiate_step(&demand, offer.clone())
        .unwrap();

    assert_eq!(result, NegotiationResult::Ready { offer });
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

fn create_demand_json(
    comp_manifest_b64: &str,
    signature_b64: Option<String>,
    signature_alg_b64: Option<&str>,
    cert_b64: Option<String>,
    cert_permissions_b64: Option<&str>,
) -> serde_json::Value {
    let mut payload = HashMap::new();
    payload.insert("@tag", json!(comp_manifest_b64));
    if signature_b64.is_some() && signature_alg_b64.is_some() {
        payload.insert(
            "sig",
            json!({
                "@tag": signature_b64.unwrap(),
                "algorithm": signature_alg_b64.unwrap().to_string()
            }),
        );
    } else if signature_b64.is_some() {
        payload.insert("sig", json!(signature_b64.unwrap()));
    } else if signature_alg_b64.is_some() {
        payload.insert(
            "sig",
            json!({ "algorithm": signature_alg_b64.unwrap().to_string() }),
        );
    }

    if cert_b64.is_some() && cert_permissions_b64.is_some() {
        payload.insert(
            "cert",
            json!({
                "@tag": cert_b64.unwrap(),
                "permissions": cert_permissions_b64.unwrap().to_string()
            }),
        );
    } else if let Some(cert_b64) = cert_b64 {
        payload.insert("cert", json!(cert_b64));
    }

    // let mut payload = manifest.to_string();
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

fn create_whitelist(whitelist_json: &str) -> DomainWhitelistState {
    let whitelist = create_whitelist_file(whitelist_json);
    let whitelist_patterns =
        DomainPatterns::load(&whitelist).expect("Can deserialize whitelist patterns");
    DomainWhitelistState::try_new(whitelist_patterns)
        .expect("Can create whitelist state from patterns")
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
