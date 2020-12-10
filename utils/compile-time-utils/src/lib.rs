use git_version::git_version;

/// Returns latest tag or version from Cargo.toml`.
pub fn git_tag() -> &'static str {
    git_version!(
        args = ["--tag", "--abbrev=0"],
        fallback = env!("CARGO_PKG_VERSION")
    )
}

/// Returns latest commit short hash.
pub fn git_rev() -> &'static str {
    env!("VERGEN_SHA_SHORT")
}

/// Returns current date in YYYY-MM-DD format.
pub fn build_date() -> &'static str {
    env!("VERGEN_BUILD_DATE")
}

/// Returns Github Actions build number if available or None.
pub fn build_number() -> Option<&'static str> {
    option_env!("GITHUB_RUN_NUMBER")
}

/// convert tag to a semantic version
pub fn semver() -> &'static str {
    let mut version = git_tag();
    for prefix in ["pre-rel-", "v"].iter() {
        if version.starts_with(prefix) {
            version = &version[prefix.len()..];
        }
    }
    version
}

#[macro_export]
macro_rules! version_describe {
    () => {
        Box::leak(
            [
                $crate::semver(),
                " (",
                $crate::git_rev(),
                " ",
                &$crate::build_date(),
                &$crate::build_number()
                    .map(|n| format!(" build #{}", n))
                    .unwrap_or("".to_string()),
                ")",
            ]
            .join("")
            .into_boxed_str(),
        ) as &str
    };
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        println!("{}", semver())
    }
}
