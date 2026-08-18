use anyhow::bail;
use std::env;
use std::ffi::OsString;

pub(crate) const ADMIN_TOKEN_ENV_VAR: &str = "YAGNA_AUTOCONF_ADMIN_TOKEN";

// A launcher is expected to generate at least 256 random bits. Length is not
// an entropy check, but rejecting shorter values prevents accidental use of a
// human password or another low-strength credential.
const MIN_ADMIN_TOKEN_BYTES: usize = 32;

pub(crate) struct StartupAdminToken(String);

impl StartupAdminToken {
    pub(crate) fn into_inner(self) -> String {
        self.0
    }
}

fn validate(value: OsString) -> anyhow::Result<StartupAdminToken> {
    let value = value
        .into_string()
        .map_err(|_| anyhow::anyhow!("{ADMIN_TOKEN_ENV_VAR} must be valid UTF-8"))?;

    if value.len() < MIN_ADMIN_TOKEN_BYTES {
        bail!("{ADMIN_TOKEN_ENV_VAR} must contain at least {MIN_ADMIN_TOKEN_BYTES} bytes");
    }

    Ok(StartupAdminToken(value))
}

/// Reads the administrator credential before dotenv processing and immediately
/// removes it from Yagna's environment so child processes cannot inherit it.
pub(crate) fn take_from_environment() -> anyhow::Result<Option<StartupAdminToken>> {
    let value = env::var_os(ADMIN_TOKEN_ENV_VAR);
    if value.is_some() {
        env::remove_var(ADMIN_TOKEN_ENV_VAR);
    }
    value.map(validate).transpose()
}

/// Removes a value which dotenv may have reintroduced. A token found only after
/// dotenv processing is rejected: administrator credentials must come from the
/// launcher's inherited environment, never from a `.env` file.
pub(crate) fn clear_after_dotenv(had_inherited_token: bool) -> anyhow::Result<()> {
    if env::var_os(ADMIN_TOKEN_ENV_VAR).is_none() {
        return Ok(());
    }

    env::remove_var(ADMIN_TOKEN_ENV_VAR);
    if !had_inherited_token {
        bail!(
            "{ADMIN_TOKEN_ENV_VAR} from a .env file is not accepted; pass it only in the child process environment"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_token_without_echoing_it() {
        let token = "do-not-print-this-token";
        let error = validate(token.into()).err().unwrap().to_string();

        assert!(error.contains(ADMIN_TOKEN_ENV_VAR));
        assert!(!error.contains(token));
    }

    #[test]
    fn accepts_256_bit_or_longer_token() {
        let token = "x".repeat(MIN_ADMIN_TOKEN_BYTES);
        assert_eq!(validate(token.clone().into()).unwrap().into_inner(), token);
    }
}
