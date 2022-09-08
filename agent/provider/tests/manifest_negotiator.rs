#[macro_use]
extern crate serial_test;

use std::collections::HashMap;
use std::path::PathBuf;
use std::{convert::TryFrom, fs};
use std::str;

use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::sign::Signer;
use serde_json::{json, Value};
use test_case::test_case;
use ya_agreement_utils::AgreementView;
use ya_manifest_test_utils::{load_certificates_from_dir, TestResources};
use ya_manifest_utils::matching::domain::{DomainPatterns, DomainWhitelistState};
use ya_manifest_utils::{Keystore, Policy, PolicyConfig};
use ya_provider::market::negotiator::builtin::ManifestSignature;
use ya_provider::market::negotiator::*;

static MANIFEST_TEST_RESOURCES: TestResources = TestResources {
    temp_dir: env!("CARGO_TARGET_TMPDIR"),
};

#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    None, // error msg
    None, // sig
    None, // sig alg
    None; // cert
    "Manifest without singature accepted because domain whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "different_domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    Some("manifest requires signature but it has none"), // error msg
    None, // sig
    None, // sig alg
    None; // cert
    "Manifest without singature rejected because domain NOT whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }, { "domain": "another.whitelisted.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com", "https://not.whitelisted.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    Some("manifest requires signature but it has none"), // error msg
    None, // sig
    None, // sig alg
    None; // cert
    "Manifest without singature rejected because ONE of domains NOT whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }] }"#, // data_dir/domain_whitelist.json
    r#"[]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    None, // error msg
    None, // sig
    None, // sig alg
    None; // cert
    "Manifest accepted because its urls list is empty"
)] 
#[test_case(
    r#"{ "patterns": [] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    None, // error msg
    Some("foo_req.key.pem"),
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"); // cert
    "Accepted manifest with url NOT whitelisted because signature valid"
)]
#[serial]
fn manifest_negotiator_test(
    whitelist: &str,
    urls: &str,
    offer: &str,
    error_msg: Option<&str>,
    signing_key: Option<&str>,
    signature_alg: Option<&str>,
    cert: Option<&str>,
) {
    // Having
    let comp_manifest_b64 = &create_comp_manifest_b64(urls);

    let whitelist_state = create_whitelist(whitelist);
    let (resource_cert_dir, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();
    
    let signature_b64 = if let Some(signing_key) = signing_key {
        let mut signing_key_path = resource_cert_dir.clone();
        signing_key_path.push(signing_key);
        let signing_key = fs::read(signing_key_path).expect("Can read signing key");
        let mut password = resource_cert_dir.clone();
        password.push("pass.txt");
        let password = fs::read(password).expect("Can read password file");
        let password = str::from_utf8(&password).unwrap().trim();
        let keypair = Rsa::private_key_from_pem_passphrase(&signing_key, password.as_bytes()).expect("Can parse signing key");
        let keypair = PKey::from_rsa(keypair).unwrap();
        let mut signer = Signer::new(MessageDigest::sha256(), &keypair).unwrap();
        signer.update(comp_manifest_b64.as_bytes()).unwrap();
        let signature = signer.sign_to_vec().expect("Can sign manifest");
        Some(base64::encode(signature))
    } else {
        None
    };

    if signing_key.is_some() {
        load_certificates_from_dir(
            &resource_cert_dir,
            &test_cert_dir,
            &["foo_ca-chain.cert.pem"],
        );
    }
    let keystore = Keystore::load(&test_cert_dir).expect("Can load test certificates");

    // TODO clean it up
    let cert_b64 = if let Some(cert) = cert {
        let mut cert_path = resource_cert_dir;
        cert_path.push(cert);
        println!("{cert_path:?}");
        let cert = fs::read(cert_path).expect("Can read cert from resources");
        Some(base64::encode(cert))
    } else {
        None
    };
    let mut config = create_manifest_signature_validating_policy_config();
    config.domain_patterns = whitelist_state;
    config.trusted_keys = Some(keystore);
    let mut manifest_negotiator = ManifestSignature::from(config);

    let demand = create_demand_json(&comp_manifest_b64, signature_b64, signature_alg, cert_b64);
    let demand: Value = serde_json::from_str(&demand).unwrap();
    let demand = AgreementView {
        json: demand,
        agreement_id: "id".to_string(),
    };
    let offer: Value = serde_json::from_str(offer).unwrap();
    let offer = AgreementView {
        json: offer,
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
) -> String {
    let mut payload = HashMap::new();
    payload.insert("@tag", json!(comp_manifest_b64));
    if signature_b64.is_some() && signature_alg_b64.is_some() {
        payload.insert(
            "sig",
            json!({
                "@tag": signature_b64.unwrap().to_string(),
                "algorithm": signature_alg_b64.unwrap().to_string()
            }),
        );
    } else if signature_b64.is_some() {
        payload.insert("sig", json!(signature_b64.unwrap().to_string()));
    } else if signature_alg_b64.is_some() {
        payload.insert(
            "sig",
            json!({ "algorithm": signature_alg_b64.unwrap().to_string() }),
        );
    }
    if let Some(cert_b64) = cert_b64 {
        payload.insert("cert", json!(cert_b64.to_string()));
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
    let demand = serde_json::to_string_pretty(&manifest).unwrap();
    println!("Tested demand:\n{demand}");
    demand
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
        DomainPatterns::try_from(&whitelist).expect("Can deserialize whitelist patterns");
    DomainWhitelistState::try_from(whitelist_patterns)
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
