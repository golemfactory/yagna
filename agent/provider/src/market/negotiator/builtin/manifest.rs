use std::collections::HashSet;
use std::ops::Not;

use ya_agreement_utils::{Error, OfferDefinition};
use ya_manifest_utils::policy::{Keystore, Match, Policy, PolicyConfig};
use ya_manifest_utils::{
    decode_manifest, AppManifest, Feature, CAPABILITIES_PROPERTY, DEMAND_MANIFEST_CERT_PROPERTY,
    DEMAND_MANIFEST_PROPERTY, DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY, DEMAND_MANIFEST_SIG_PROPERTY,
};

use crate::market::negotiator::*;

#[derive(Default)]
pub struct ManifestSignature {
    enabled: bool,
    keystore: Keystore,
    domain_whitelist: HashSet<String>,
}

impl NegotiatorComponent for ManifestSignature {
    fn negotiate_step(
        &mut self,
        demand: &ProposalView,
        offer: ProposalView,
    ) -> anyhow::Result<NegotiationResult> {
        if self.enabled.not() {
            return Ok(NegotiationResult::Ready { offer });
        }

        let (manifest, manifest_encoded) =
            match demand.get_property::<String>(DEMAND_MANIFEST_PROPERTY) {
                Err(Error::NoKey(_)) => return Ok(NegotiationResult::Ready { offer }),
                Err(e) => return rejection(format!("invalid manifest type: {:?}", e)),
                Ok(manifest_encoded) => match decode_manifest(&manifest_encoded) {
                    Ok(manifest) => (manifest, manifest_encoded),
                    Err(e) => return rejection(format!("invalid manifest: {:?}", e)),
                },
            };

        if manifest.features().is_empty() {
            return Ok(NegotiationResult::Ready { offer });
        }

        match self.verify_manifest(demand, manifest_encoded, manifest) {
            Err(err) => rejection(format!("failed to verify manifest signature: {}", err)),
            Ok(()) => Ok(NegotiationResult::Ready { offer }),
        }
    }

    fn fill_template(
        &mut self,
        offer_template: OfferDefinition,
    ) -> anyhow::Result<OfferDefinition> {
        Ok(offer_template)
    }

    fn on_agreement_terminated(
        &mut self,
        _agreement_id: &str,
        _result: &AgreementResult,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn on_agreement_approved(&mut self, _agreement_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

impl From<PolicyConfig> for ManifestSignature {
    fn from(config: PolicyConfig) -> Self {
        let policies = config.policy_set();
        let properties = config.trusted_property_map();

        let enabled = if policies
            .contains(&Policy::ManifestSignatureValidation)
            .not()
        {
            false
        } else {
            match properties.get(CAPABILITIES_PROPERTY) {
                Some(Match::Values(vec)) => vec.contains(&Feature::Inet.to_string()).not(),
                Some(Match::All) => false,
                _ => true,
            }
        };

        ManifestSignature {
            enabled,
            keystore: config.trusted_keys.unwrap_or_default(),
            domain_whitelist: config.domain_whitelist,
        }
    }
}

impl ManifestSignature {
    /// Verifies fields base64 encoding, then validates certificate, then validates signature, then verifies manifest content and returns it
    fn verify_manifest<S: AsRef<str>>(
        &self,
        demand: &ProposalView,
        manifest_encoded: S,
        manifest: AppManifest,
    ) -> anyhow::Result<()> {
        let sig = match demand.get_property::<String>(DEMAND_MANIFEST_SIG_PROPERTY) {
            Ok(sig) => sig,
            Err(Error::NoKey(_)) => return self.verify_if_inet_out_urls_whitelisted(manifest),
            Err(e) => anyhow::bail!(format!("invalid manifest signature type: {:?}", e)),
        };
        log::trace!("sig_hex: {}", sig);
        let sig_alg: String = demand.get_property(DEMAND_MANIFEST_SIG_ALGORITHM_PROPERTY)?;
        log::trace!("sig_alg: {}", sig_alg);
        let cert: String = demand.get_property(DEMAND_MANIFEST_CERT_PROPERTY)?;
        log::trace!("cert: {}", cert);
        log::trace!("manifest: {}", manifest_encoded.as_ref());
        self.keystore
            .verify_signature(cert, sig, sig_alg, manifest_encoded)
    }

    fn verify_if_inet_out_urls_whitelisted(&self, manifest: AppManifest) -> anyhow::Result<()> {
        if let Some(urls) = manifest
            .comp_manifest
            .and_then(|comp_manifest| comp_manifest.net)
            .and_then(|net| net.inet)
            .and_then(|inet| inet.out)
            .and_then(|inet_out| inet_out.urls)
        {
            let non_whitelisted_urls: HashSet<String> = urls
                .iter()
                .flat_map(url::Url::domain)
                .map(str::to_string)
                .filter(|domain| self.domain_whitelist.contains(domain).not())
                .collect();
            if non_whitelisted_urls.is_empty() {
                return Ok(());
            }
            anyhow::bail!(
                "no signed manifest with non whitelisted domains: {non_whitelisted_urls:?}"
            )
        }
        anyhow::bail!("no manifest signature")
    }
}

fn rejection(message: String) -> anyhow::Result<NegotiationResult> {
    Ok(NegotiationResult::Reject {
        message,
        is_final: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use structopt::StructOpt;

    fn build_policy<S: AsRef<str>>(args: S) -> ManifestSignature {
        let arguments = shlex::split(args.as_ref()).expect("failed to parse arguments");
        PolicyConfig::from_iter(arguments).into()
    }

    #[test]
    fn parse_signature_policy() {
        let policy = build_policy(
            "TEST \
            --policy-disable-component all \
            --policy-trust-property property=value1,value2",
        );
        assert!(!policy.enabled);

        let policy = build_policy(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property property=value1,value2",
        );
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property {}",
            CAPABILITIES_PROPERTY
        ));
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-disable-component manifest_signature_validation \
            --policy-trust-property {}={}",
            CAPABILITIES_PROPERTY,
            Feature::Inet.to_string()
        ));
        assert!(!policy.enabled);

        let policy = build_policy(format!(
            "TEST \
            --policy-trust-property {}={}",
            CAPABILITIES_PROPERTY,
            Feature::Inet.to_string()
        ));
        assert!(!policy.enabled);

        let policy = build_policy(&format!(
            "TEST \
            --policy-trust-property {}",
            CAPABILITIES_PROPERTY
        ));
        assert!(!policy.enabled);

        let policy = build_policy(
            "TEST \
            --policy-trust-property property=value1,value2",
        );
        assert!(policy.enabled);

        let policy = build_policy(
            "TEST \
            --policy-trust-property property",
        );
        assert!(policy.enabled);

        let policy = build_policy("TEST");
        assert!(policy.enabled);
    }
}
