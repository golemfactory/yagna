use crate::api::{have_consent, set_consent};
use crate::model::full_question;
use crate::ConsentType;
use anyhow::anyhow;
use strum::IntoEnumIterator;

pub fn consent_check_before_startup(interactive: bool) -> anyhow::Result<()> {
    // if feature require-consent is enabled, skip check
    if cfg!(feature = "require-consent") {
        log::info!("Checking consents before startup {}", interactive);
        for consent_type in ConsentType::iter() {
            let consent_int = have_consent(consent_type, true);
            if consent_int.consent.is_none() {
                let res = loop {
                    let prompt_res = if interactive {
                        match promptly::prompt_default(
                            format!("{} [allow/deny]", full_question(consent_type)),
                            "allow".to_string(),
                        ) {
                            Ok(res) => res,
                            Err(err) => {
                                return Err(anyhow!(
                                    "Error when prompting: {}. Run setup again.",
                                    err
                                ));
                            }
                        }
                    } else {
                        log::warn!("Consent {} not set. Run installer again or run command yagna consent allow {}",
                               consent_type,
                               consent_type.to_lowercase_str());
                        return Ok(());
                    };
                    if prompt_res == "allow" {
                        break true;
                    } else if prompt_res == "deny" {
                        break false;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                };
                set_consent(consent_type, Some(res));
            }
        }

        for consent_type in ConsentType::iter() {
            let consent_res = have_consent(consent_type, false);
            if let Some(consent) = consent_res.consent {
                log::info!(
                    "Consent {} - {} ({})",
                    consent_type,
                    if consent { "allow" } else { "deny" },
                    consent_res.source
                );
            };
        }
    }
    Ok(())
}
