use strum::IntoEnumIterator;
use crate::api::{have_consent, set_consent};
use crate::ConsentType;

pub fn consent_check_before_startup() -> anyhow::Result<()> {
    for consent_type in ConsentType::iter() {
        let consent_int = have_consent(consent_type);
        if consent_int.is_none() {
            let res = loop {
                let propt_res = promptly::prompt_default("Allow for internal monitoring [allow/deny]", "allow".to_string()).unwrap_or("".to_string());
                if propt_res == "allow" {
                    break true;
                } else if propt_res == "deny" {
                    break false;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            };
            if res {
                set_consent(ConsentType::Internal, Some(res));
            }
        }
    }

    Ok(())
}