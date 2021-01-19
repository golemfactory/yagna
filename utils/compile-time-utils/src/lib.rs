use git_version::git_version;
use semver::Version;

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
pub fn build_number_str() -> Option<&'static str> {
    option_env!("GITHUB_RUN_NUMBER")
}

pub fn build_number() -> Option<u64> {
    build_number_str().map(|i| {
        // should not panic
        i.parse().unwrap()
    })
}

/// convert tag to a semantic version
pub fn semver_str() -> &'static str {
    let mut version = git_tag();
    for prefix in ["pre-rel-", "v"].iter() {
        if version.starts_with(prefix) {
            version = &version[prefix.len()..];
        }
    }
    version
}

pub fn semver() -> Version {
    // It must parse correctly and if it passes test it won't change later.
    Version::parse(semver_str()).unwrap()
}

#[macro_export]
macro_rules! version_describe {
    () => {
        Box::leak(
            [
                $crate::semver_str(),
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
    fn test_semver() {
        // should not panic
        semver();
    }

    #[test]
    fn test_build_number() {
        // should not panic
        build_number();
    }
}
