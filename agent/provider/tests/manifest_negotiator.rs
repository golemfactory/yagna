#[macro_use]
extern crate serial_test;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};
use test_case::test_case;
use ya_agreement_utils::AgreementView;
use ya_manifest_test_utils::{load_certificates_from_dir, TestResources};
use ya_manifest_utils::matching::domain::{DomainPatterns, DomainWhitelistState};
use ya_manifest_utils::policy::CertPermissions;
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
    None, // private key file
    None, // sig alg
    None, // cert
    None; // error msg
    "Manifest without signature accepted because domain whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "do.*ain.com", "type": "regex" }, { "domain": "another.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    None, // private key file
    None, // sig alg
    None, // cert
    None; // error msg
    "Manifest without signature accepted because domain whitelisted (regex pattern)"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "different_domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    None, // private key file
    None, // sig alg
    None, // cert
    Some("manifest requires signature but it has none"); // error msg
    "Manifest without signature rejected because domain NOT whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }, { "domain": "another.whitelisted.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com", "https://not.whitelisted.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    None, // private key file
    None, // sig alg
    None, // cert
    Some("manifest requires signature but it has none"); // error msg
    "Manifest without signature rejected because ONE of domains NOT whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }] }"#, // data_dir/domain_whitelist.json
    r#"[]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    None, // private key file
    None, // sig alg
    None, // cert
    None; // error msg
    "Manifest accepted because its urls list is empty"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "regex" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    None; // error msg
    "Manifest accepted with url NOT whitelisted because signature valid"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("dummy_inter.cert.pem"), // untrusted cert
    Some("failed to verify manifest signature: Invalid certificate"); // error msg
    "Manifest rejected because of invalid certificate even when urls domains are whitelisted"
)]
#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    Some("foo_inter.key.pem"), // private key file not matching certificate
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // certificate not matching private key
    Some("failed to verify manifest signature: Invalid signature"); // error msg
    "Manifest rejected because of invalid signature (signed using incorrect private key) even when urls domains are whitelisted"
)]
#[serial]
fn manifest_negotiator_test(
    whitelist: &str,
    urls: &str,
    offer: &str,
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

    manifest_negotiator_test_encoded_manifest_sign_and_cert(
        whitelist,
        comp_manifest_b64,
        offer,
        signature_b64,
        signature_alg,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
    )
}

#[test_case(
    r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#, // data_dir/domain_whitelist.json
    r#"["https://domain.com"]"#, // compManifest.net.inet.out.urls
    r#"{ "any": "thing" }"#, // offer
    Some("broken_signature"), // signature (broken)
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    Some("failed to verify manifest signature: Invalid signature"); // error msg
    "Manifest rejected because of invalid signature"
)]
#[serial]
fn manifest_negotiator_test_encoded_sign_and_cert(
    whitelist: &str,
    urls: &str,
    offer: &str,
    signature_b64: Option<&str>,
    signature_alg: Option<&str>,
    cert: Option<&str>,
    error_msg: Option<&str>,
) {
    let comp_manifest_b64 = create_comp_manifest_b64(urls);
    let signature_b64 = signature_b64.map(|signature| signature.to_string());

    let cert_b64 = cert.map(cert_file_to_cert_b64);
    manifest_negotiator_test_encoded_manifest_sign_and_cert(
        whitelist,
        comp_manifest_b64,
        offer,
        signature_b64,
        signature_alg,
        cert_b64,
        None,
        error_msg,
        &vec![CertPermissions::All],
    )
}

#[test_case(
    r#"{ "any": "thing" }"#, // offer
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    None, // cert_permissions_b64
    &vec![CertPermissions::OutboundManifest],
    None;
    "Manifest accepted, because permissions are sufficient"
)]
#[test_case(
    r#"{ "any": "thing" }"#, // offer
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    None, // cert_permissions_b64
    &vec![CertPermissions::All],
    None;
    "Manifest accepted, when permissions are set to `All`"
)]
#[test_case(
    r#"{ "any": "thing" }"#, // offer
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    None, // cert_permissions_b64
    &vec![],
    Some("certificate permissions verification: Not sufficient permissions. Required: `outbound-manifest`, but has only: `none`"); // error msg
    "Manifest rejected, because certificate has no permissions"
)]
#[test_case(
    r#"{ "any": "thing" }"#, // offer
    Some("foo_inter.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_inter.cert.pem"), // cert
    None, // cert_permissions_b64
    &vec![CertPermissions::OutboundManifest], // certs_permissions
    Some("certificate permissions verification: Not sufficient permissions. Required: `outbound-manifest`, but has only: `none`"); // error msg
    "Manifest rejected, because parent certificate has no permissions"
)]
#[test_case(
    r#"{ "any": "thing" }"#, // offer
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    Some("NYI"), // cert_permissions_b64
    &vec![CertPermissions::OutboundManifest, CertPermissions::UnverifiedPermissionsChain],
    None;
    "Manifest accepted, because permissions are sufficient (has `unverified-permissions-chain` permission)"
)]
#[test_case(
    r#"{ "any": "thing" }"#, // offer
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    Some("NYI"), // cert_permissions_b64
    &vec![CertPermissions::OutboundManifest],
    Some("certificate permissions verification: Not sufficient permissions. Required: `outbound-manifest|unverified-permissions-chain`, but has only: `outbound-manifest`"); // error msg
    "Manifest rejected, because certificate has no `unverified-permissions-chain` permission."
)]
#[test_case(
    r#"{ "any": "thing" }"#, // offer
    Some("foo_req.key.pem"), // private key file
    Some("sha256"), // sig alg
    Some("foo_req.cert.pem"), // cert
    Some("NYI"), // cert_permissions_b64
    &vec![CertPermissions::All],
    Some("certificate permissions verification: Not sufficient permissions. Required: `outbound-manifest|unverified-permissions-chain`, but has only: `all`"); // error msg
    "Manifest rejected, even when permissions are set to `All` because `unverified-permissions-chain` permission is also required when Demand comes with its permissions"
)]
#[serial]
fn test_manifest_negotiator_certs_permissions(
    offer: &str,
    signing_key: Option<&str>,
    signature_alg: Option<&str>,
    cert: Option<&str>,
    cert_permissions_b64: Option<&str>,
    certs_permissions: &Vec<CertPermissions>,
    error_msg: Option<&str>,
) {
    let comp_manifest_b64 = create_comp_manifest_b64(r#"["https://domain.com"]"#);

    let signature_b64 = signing_key.map(|signing_key| {
        MANIFEST_TEST_RESOURCES.sign_data(comp_manifest_b64.as_bytes(), signing_key)
    });

    let cert_b64 = cert.map(cert_file_to_cert_b64);
    manifest_negotiator_test_encoded_manifest_sign_and_cert(
        r#"{ "patterns": [{ "domain": "domain.com", "type": "strict" }] }"#,
        comp_manifest_b64,
        offer,
        signature_b64,
        signature_alg,
        cert_b64,
        cert_permissions_b64,
        error_msg,
        certs_permissions,
    )
}

#[allow(clippy::too_many_arguments)]
fn manifest_negotiator_test_encoded_manifest_sign_and_cert(
    whitelist: &str,
    comp_manifest_b64: String,
    offer: &str,
    signature_b64: Option<String>,
    signature_alg: Option<&str>,
    cert_b64: Option<String>,
    cert_permissions_b64: Option<&str>,
    error_msg: Option<&str>,
    certs_permissions: &Vec<CertPermissions>,
) {
    // Having
    let whitelist_state = create_whitelist(whitelist);
    let (resource_cert_dir, test_cert_dir) = MANIFEST_TEST_RESOURCES.init_cert_dirs();

    if signature_b64.is_some() {
        load_certificates_from_dir(
            &resource_cert_dir,
            &test_cert_dir,
            &["foo_ca-chain.cert.pem"],
            certs_permissions,
        );
    }
    let keystore = Keystore::load(&test_cert_dir).expect("Can load test certificates");

    let mut config = create_manifest_signature_validating_policy_config();
    config.domain_patterns = whitelist_state;
    config.trusted_keys = Some(keystore);
    let mut manifest_negotiator = ManifestSignature::from(config);
    // Current implementation does not verify content of certificate permissions incoming in demand.

    let demand = create_demand_json(
        &comp_manifest_b64,
        signature_b64,
        signature_alg,
        cert_b64,
        cert_permissions_b64,
    );
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
) -> String {
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
