use crate::api::{have_consent, set_consent};
use crate::model::full_question;
use crate::ConsentType;
use strum::IntoEnumIterator;

pub fn consent_check_before_startup(interactive: bool) -> anyhow::Result<()> {
    // if feature consent-always-allow is enabled, skip check
    if cfg!(feature = "consent-always-allow") {
        return Ok(());
    }

    for consent_type in ConsentType::iter() {
        let consent_int = have_consent(consent_type);
        if consent_int.is_none() {
            let res = loop {
                let propt_res = if interactive {
                    promptly::prompt_default(
                        format!("{} [allow/deny]", full_question(consent_type)),
                        "allow".to_string(),
                    )
                    .unwrap_or("".to_string())
                } else {
                    "allow".to_string()
                };
                if propt_res == "allow" {
                    break true;
                } else if propt_res == "deny" {
                    break false;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            };
            set_consent(consent_type, Some(res));
        }
    }

    for consent_type in ConsentType::iter() {
        let consent_int = have_consent(consent_type);
        if let Some(consent) = consent_int {
            log::info!(
                "Consent {} - {}",
                consent_type,
                if consent { "allow" } else { "deny" }
            );
        };
    }
    Ok(())
}
