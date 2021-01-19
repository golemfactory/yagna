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
pub fn build_number() -> Option<&'static str> {
    option_env!("GITHUB_RUN_NUMBER")
}

/// convert tag to a semantic version
pub fn semver_str() -> String {
    let mut version = git_tag();
    for prefix in ["pre-rel-", "v"].iter() {
        if version.starts_with(prefix) {
            version = &version[prefix.len()..];
        }
    }
    if let Some(bn) = build_number() {
        [version, bn].join("+")
    } else {
        version.to_string()
    }
}

pub fn semver() -> Version {
    Version::parse(&semver_str()).unwrap()
}

pub fn report_version_to_metrics() {
    let version = semver();
    value!("yagna.version.major", version.major);
    value!("yagna.version.minor", version.minor);
    value!("yagna.version.patch", version.patch);
    value!(
        "yagna.version.is_prerelease",
        (!version.pre.is_empty()) as u64
    );
    if !version.build.is_empty() {
        if let semver::Identifier::Numeric(build_number) = version.build[0] {
            value!("yagna.version.build_number", build_number);
        }
    }
}

#[macro_export]
macro_rules! version_describe {
    () => {
        Box::leak(
            [
                &$crate::semver_str(),
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
    fn test_git_rev() {
        println!("git rev: {:?}", git_rev());
    }

    #[test]
    fn test_semver() {
        // should not panic
        println!("semver: {:?}", semver());
    }

    #[test]
    fn test_build_number() {
        // should not panic
        println!("build: {:?}", build_number());
    }
}
