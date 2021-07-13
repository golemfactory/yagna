pub use git_version::git_version;
use metrics::gauge;
use semver::Version;

/// Returns latest commit short hash.
pub fn git_rev() -> &'static str {
    env!("VERGEN_SHA_SHORT")
}

/// Returns current date in YYYY-MM-DD format.
pub fn build_date() -> &'static str {
    env!("VERGEN_BUILD_DATE")
}

/// Returns Github Actions build string if available or None.
pub fn build_number_str() -> Option<&'static str> {
    option_env!("GITHUB_RUN_NUMBER")
}

/// Returns Github Actions build number if available or None.
pub fn build_number() -> Option<i64> {
    build_number_str().map(|s| s.parse().ok()).flatten()
}

/// Converts a tag to semantic version
pub fn tag2semver(tag: &str) -> &str {
    let mut version = tag;
    for prefix in ["pre-rel-", "v"].iter() {
        if version.starts_with(prefix) {
            version = &version[prefix.len()..];
        }
    }
    version
}

pub fn report_version_to_metrics() {
    if let Ok(version) = Version::parse(semver_str!()) {
        gauge!("yagna.version.major", version.major as i64);
        gauge!("yagna.version.minor", version.minor as i64);
        gauge!("yagna.version.patch", version.patch as i64);
        gauge!(
            "yagna.version.is_prerelease",
            (!version.pre.is_empty()) as i64
        );
        if let Some(build_number) = build_number() {
            gauge!("yagna.version.build_number", build_number);
        }
    }
}

/// Returns latest version tag
#[macro_export]
macro_rules! git_tag {
    () => {
        $crate::git_version!(
            args = [
                "--tag",
                "--abbrev=0",
                "--match=v[0-9]*",
                "--match=pre-rel-v[0-9]*"
            ],
            cargo_prefix = ""
        )
    };
}

/// Returns a semantic version string of the crate
#[macro_export]
macro_rules! semver_str {
    () => {
        $crate::tag2semver($crate::git_tag!())
    };
}

#[macro_export]
macro_rules! version_describe {
    () => {
        Box::leak(
            [
                $crate::semver_str!(),
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
    fn test_git_tag() {
        println!("git tag: {:?}", git_tag!());
    }

    #[test]
    fn test_git_rev() {
        println!("git rev: {:?}", git_rev());
    }

    #[test]
    fn test_semver() {
        println!("semver: {:?}", Version::parse(semver_str!()));
    }

    #[test]
    fn test_build_number() {
        println!("build: {:?}", build_number());
    }
}
