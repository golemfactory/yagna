#[macro_use]
extern crate serial_test;

use std::path::PathBuf;
use std::{convert::TryFrom, fs};

use serde_json::Value;
use test_case::test_case;
use ya_agreement_utils::AgreementView;
use ya_manifest_utils::matching::domain::{DomainPatterns, DomainWhitelistState};
use ya_manifest_utils::{Keystore, Policy, PolicyConfig};
use ya_provider::market::negotiator::builtin::ManifestSignature;
use ya_provider::market::negotiator::*;

#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, 
    r#"{
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
                        "urls": ["https://domain.com"]
                    }
                }
            }
        }
    }"#, 
    r#"{ "any": "thing" }"#,
    None;
    "Manifest without singature accepted because domain whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "different_domain.com", "type": "strict" }] }"#, 
    r#"{
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
                        "urls": ["https://domain.com"]
                    }
                }
            }
        }
    }"#, 
    r#"{ "any": "thing" }"#,
    Some("manifest requires signature but it has none");
    "Manifest without singature rejected because domain NOT whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }, { "domain": "another.whitelisted.com", "type": "strict" }] }"#, 
    r#"{
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
                        "urls": ["https://domain.com", "https://not.whitelisted.com"]
                    }
                }
            }
        }
    }"#, 
    r#"{ "any": "thing" }"#,
    Some("manifest requires signature but it has none");
    "Manifest without singature rejected because ONE of domains NOT whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }] }"#, 
    r#"{
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
                        "urls": []
                    }
                }
            }
        }
    }"#, 
    r#"{ "any": "thing" }"#,
    None;
    "Manifest accepted because its urls list is empty"
)]
#[serial]
fn manifest_negotiator_test(
    whitelist: &str,
    comp_manifest: &str,
    offer: &str,
    error_msg: Option<&str>,
) {
    // Having
    let whitelist_state = create_whitelist(whitelist);
    let keystore = create_empty_keystore();
    let mut config = create_manifest_signature_validating_policy_config();
    config.domain_patterns = whitelist_state;
    config.trusted_keys = Some(keystore);
    let mut manifest_negotiator = ManifestSignature::from(config);

    let demand = create_demand_json(comp_manifest);
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

fn create_demand_json(manifest: &str) -> String {
    let manifest_base64 = base64::encode(manifest);
    format!(
        "{{ \"golem\": {{ \"srv\" : {{ \"comp\": {{ \"payload\": \"{manifest_base64}\" }} }} }} }}"
    )
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

fn create_empty_keystore() -> Keystore {
    let cert_dir = cert_dir();
    if cert_dir.exists() {
        fs::remove_dir_all(&cert_dir).expect("Can delete temp cert dir");
    }
    fs::create_dir(cert_dir.as_path()).expect("Can create temp cert dir");
    Keystore::load(&cert_dir).expect("Can create empty keystore")
}

fn cert_dir() -> PathBuf {
    tmp_resource("cert_dir")
}

fn tmp_resource(name: &str) -> PathBuf {
    let mut resource = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    resource.push(name);
    resource
}
