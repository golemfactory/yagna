use git_version::git_version;
use metrics::value;
use semver::Version;

/// Returns latest tag (via `git describe --tag --abbrev=0`) or version from Cargo.toml`.
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
    build_number_str().map(|s| s.parse().ok()).flatten()
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

pub fn semver() -> std::result::Result<Version, semver::SemVerError> {
    Version::parse(semver_str())
}

pub fn report_version_to_metrics() {
    if let Ok(version) = semver() {
        value!("yagna.version.major", version.major);
        value!("yagna.version.minor", version.minor);
        value!("yagna.version.patch", version.patch);
        value!(
            "yagna.version.is_prerelease",
            (!version.pre.is_empty()) as u64
        );
        if let Some(build_number) = build_number() {
            value!("yagna.version.build_number", build_number);
        }
    }
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
                &$crate::build_number_str()
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
    fn test_git_rev() {
        println!("git rev: {:?}", git_rev());
    }

    #[test]
    fn test_semver() {
        println!("semver: {:?}", semver());
    }

    #[test]
    fn test_build_number() {
        println!("build: {:?}", build_number());
    }
}
