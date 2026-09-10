#![allow(non_local_definitions)] // Due to Diesel macros.

#[macro_use]
extern crate diesel;

mod cdn;
mod db;
mod notifier;
mod service;

pub use service::VersionService;

fn version_is_greater(current: &str, other: &str) -> Result<bool, semver::Error> {
    Ok(semver::Version::parse(other)? > semver::Version::parse(current)?)
}

#[cfg(test)]
mod tests {
    use super::version_is_greater;

    #[test]
    fn compares_semantic_versions() {
        assert!(version_is_greater("0.17.5", "0.17.6").unwrap());
        assert!(!version_is_greater("0.17.6", "0.17.6").unwrap());
        assert!(!version_is_greater("0.17.6", "0.17.5").unwrap());
        assert!(version_is_greater("0.17.6-rc.1", "0.17.6").unwrap());
        assert!(version_is_greater("invalid", "0.17.6").is_err());
    }
}
