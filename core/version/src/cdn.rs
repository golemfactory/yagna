use std::convert::TryFrom;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use chrono::{DateTime, FixedOffset};
use metrics::counter;
use reqwest::Client;
use serde::Deserialize;
use ya_core_model::version::Release;
use ya_persistence::executor::DbExecutor;

use crate::db::dao::ReleaseDAO;
use crate::db::model::DBRelease;
use crate::service::cli::ReleaseMessage;
use crate::version_is_greater;

const LATEST_RELEASE_URL: &str = "https://golem-releases.cdn.golem.network/yagna/LATEST";
const MAX_RELEASE_MANIFEST_SIZE: usize = 16 * 1024;
const RELEASE_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(crate) struct CdnRelease {
    pub version: String,
    pub name: String,
    pub released_at: DateTime<FixedOffset>,
}

#[derive(Deserialize)]
struct ReleaseManifest {
    version: String,
    name: String,
    released_at: String,
}

fn parse_release_manifest(body: &[u8]) -> anyhow::Result<CdnRelease> {
    if body.len() > MAX_RELEASE_MANIFEST_SIZE {
        bail!(
            "Yagna release manifest is too large: {} bytes (maximum: {})",
            body.len(),
            MAX_RELEASE_MANIFEST_SIZE
        );
    }

    let manifest: ReleaseManifest =
        serde_json::from_slice(body).context("Invalid Yagna release manifest JSON")?;
    let version = semver::Version::parse(&manifest.version)
        .with_context(|| format!("Invalid Yagna release version `{}`", manifest.version))?;
    if !version.pre.is_empty() {
        bail!("Yagna LATEST manifest points to a prerelease: {version}");
    }
    let released_at = DateTime::parse_from_rfc3339(&manifest.released_at)
        .with_context(|| format!("Invalid Yagna release timestamp `{}`", manifest.released_at))?;

    Ok(CdnRelease {
        version: version.to_string(),
        name: manifest.name,
        released_at,
    })
}

async fn fetch_latest_release() -> anyhow::Result<CdnRelease> {
    let client = Client::builder()
        .user_agent(concat!("yagna/", env!("CARGO_PKG_VERSION")))
        .timeout(RELEASE_REQUEST_TIMEOUT)
        .build()?;
    let mut response = client
        .get(LATEST_RELEASE_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await?
        .error_for_status()?;

    if response
        .content_length()
        .map(|length| length > MAX_RELEASE_MANIFEST_SIZE as u64)
        .unwrap_or(false)
    {
        bail!(
            "Yagna release manifest exceeds the maximum size of {} bytes",
            MAX_RELEASE_MANIFEST_SIZE
        );
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let new_size = body
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| anyhow!("Yagna release manifest size overflow"))?;
        if new_size > MAX_RELEASE_MANIFEST_SIZE {
            bail!(
                "Yagna release manifest exceeds the maximum size of {} bytes",
                MAX_RELEASE_MANIFEST_SIZE
            );
        }
        body.extend_from_slice(&chunk);
    }

    parse_release_manifest(&body)
}

pub async fn check_latest_release(db: &DbExecutor) -> anyhow::Result<Release> {
    log::debug!("Checking latest Yagna release via CDN");
    let cdn_release = fetch_latest_release().await?;

    log::trace!("Got latest Yagna release: {cdn_release:?}");

    let db_release = DBRelease::try_from(cdn_release)?;
    let release = match db.as_dao::<ReleaseDAO>().save_new(db_release.clone()).await {
        Err(error) => {
            let release = db_release.into();
            log::error!("Storing new Yagna release {release} to DB: {error}");
            release
        }
        Ok(release) => release,
    };

    if version_is_greater(ya_compile_time_utils::semver_str!(), &release.version).map_err(
        |error| {
            anyhow!(
                "CDN release version `{}` parse error: {}",
                release.version,
                error
            )
        },
    )? {
        counter!("version.new", 1);
        log::warn!("{}", ReleaseMessage::Available(&release));
    }
    Ok(release)
}

pub(crate) async fn store_running_release(db: &DbExecutor) -> anyhow::Result<Release> {
    if let Some(release) = db.as_dao::<ReleaseDAO>().current_release().await? {
        return Ok(release);
    }

    let db_release = DBRelease::current()?;
    let release = match db.as_dao::<ReleaseDAO>().save_new(db_release.clone()).await {
        Err(error) => {
            let release = db_release.into();
            log::error!("Storing running Yagna release {release} to DB: {error}");
            release
        }
        Ok(release) => {
            log::info!("Stored currently running Yagna release {release} in DB");
            release
        }
    };
    Ok(release)
}

#[cfg(test)]
mod tests {
    use super::{parse_release_manifest, LATEST_RELEASE_URL, MAX_RELEASE_MANIFEST_SIZE};

    const VALID_MANIFEST: &str = r#"{
        "version": "0.17.8",
        "name": "v0.17.8 #1465",
        "released_at": "2026-07-31T08:28:47Z"
    }"#;

    #[test]
    fn uses_cdn_latest_url() {
        assert_eq!(
            LATEST_RELEASE_URL,
            "https://golem-releases.cdn.golem.network/yagna/LATEST"
        );
    }

    #[test]
    fn parses_valid_release_manifest() {
        let release = parse_release_manifest(VALID_MANIFEST.as_bytes()).unwrap();

        assert_eq!(release.version, "0.17.8");
        assert_eq!(release.name, "v0.17.8 #1465");
        assert_eq!(
            release.released_at.to_rfc3339(),
            "2026-07-31T08:28:47+00:00"
        );
    }

    #[test]
    fn rejects_oversized_release_manifest() {
        let body = vec![b' '; MAX_RELEASE_MANIFEST_SIZE + 1];
        let error = parse_release_manifest(&body).unwrap_err().to_string();

        assert!(error.contains("too large"));
    }

    #[test]
    fn rejects_invalid_json() {
        let error = parse_release_manifest(b"not json").unwrap_err().to_string();

        assert!(error.contains("Invalid Yagna release manifest JSON"));
    }

    #[test]
    fn rejects_invalid_or_prefixed_version() {
        for version in ["v0.17.8", "invalid"] {
            let manifest = VALID_MANIFEST.replace("0.17.8", version);
            let error = parse_release_manifest(manifest.as_bytes())
                .unwrap_err()
                .to_string();

            assert!(error.contains("Invalid Yagna release version"));
        }
    }

    #[test]
    fn rejects_prerelease_version() {
        let manifest = VALID_MANIFEST.replace("0.17.8", "0.18.0-rc.1");
        let error = parse_release_manifest(manifest.as_bytes())
            .unwrap_err()
            .to_string();

        assert!(error.contains("points to a prerelease"));
    }

    #[test]
    fn rejects_invalid_release_timestamp() {
        let manifest = VALID_MANIFEST.replace("2026-07-31T08:28:47Z", "2026-07-31");
        let error = parse_release_manifest(manifest.as_bytes())
            .unwrap_err()
            .to_string();

        assert!(error.contains("Invalid Yagna release timestamp"));
    }
}
