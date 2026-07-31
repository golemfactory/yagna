use anyhow::anyhow;
use metrics::counter;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::convert::TryFrom;

use ya_core_model::version::Release;
use ya_persistence::executor::DbExecutor;

use crate::db::dao::ReleaseDAO;
use crate::db::model::DBRelease;
use crate::service::cli::ReleaseMessage;
use crate::version_is_greater;

const REPO_OWNER: &str = "golemfactory";
const REPO_NAME: &str = "yagna";

#[derive(Debug)]
pub(crate) struct GitHubRelease {
    pub version: String,
    pub name: String,
    pub date: String,
}

#[derive(Deserialize)]
struct GitHubApiRelease {
    tag_name: String,
    name: Option<String>,
    created_at: String,
}

fn release_url(tag: Option<&str>) -> anyhow::Result<reqwest::Url> {
    let mut url = reqwest::Url::parse("https://api.github.com")?;
    let mut path = url
        .path_segments_mut()
        .map_err(|_| anyhow!("GitHub API URL cannot be a base"))?;
    path.extend(["repos", REPO_OWNER, REPO_NAME, "releases"]);
    match tag {
        Some(tag) => path.extend(["tags", tag]),
        None => path.push("latest"),
    };
    drop(path);
    Ok(url)
}

fn fetch_release(tag: Option<&str>) -> anyhow::Result<GitHubRelease> {
    let release = Client::builder()
        .user_agent(concat!("yagna/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(release_url(tag)?)
        .send()?
        .error_for_status()?
        .json::<GitHubApiRelease>()?;
    let GitHubApiRelease {
        tag_name,
        name,
        created_at,
    } = release;
    let name = name.unwrap_or_else(|| tag_name.to_owned());

    Ok(GitHubRelease {
        version: tag_name.trim_start_matches('v').to_owned(),
        name,
        date: created_at,
    })
}

pub async fn check_latest_release(db: &DbExecutor) -> anyhow::Result<Release> {
    log::debug!("Checking latest Yagna release");
    let gh_rel = tokio::task::spawn_blocking(|| fetch_release(None)).await??;

    log::trace!("Got latest Yagna release {:?}", gh_rel);

    let db_rel = DBRelease::try_from(gh_rel)?;
    let rel = match db.as_dao::<ReleaseDAO>().save_new(db_rel.clone()).await {
        Err(e) => {
            let r = db_rel.into();
            log::error!("Storing new Yagna release {} to DB. {}", r, e);
            r
        }
        Ok(r) => r,
    };

    if version_is_greater(ya_compile_time_utils::semver_str!(), &rel.version).map_err(|e| {
        anyhow!(
            "Github release version `{}` parse error: {}",
            rel.version,
            e
        )
    })? {
        counter!("version.new", 1);
        log::warn!("{}", ReleaseMessage::Available(&rel));
    };
    Ok(rel)
}

pub(crate) async fn check_running_release(db: &DbExecutor) -> anyhow::Result<Release> {
    if let Some(release) = db.as_dao::<ReleaseDAO>().current_release().await? {
        return Ok(release);
    }

    let running_tag = ya_compile_time_utils::git_tag!();
    log::debug!("Checking release for running tag: {}", running_tag);

    if running_tag.contains("-rc") {
        log::trace!("Currently running Yagna rc release. Not stored in DB.");

        return Ok(DBRelease::current()?.into());
    }

    let db_rel = match tokio::task::spawn_blocking(move || fetch_release(Some(running_tag))).await?
    {
        Ok(gh_rel) => {
            log::trace!("Got currently running release: {:?}", gh_rel);
            DBRelease::try_from(gh_rel)?
        }
        Err(e) => {
            log::trace!(
                "Failed to get release for running tag: '{}': {}. Using current",
                running_tag,
                e
            );
            DBRelease::current()?
        }
    };

    let rel = match db.as_dao::<ReleaseDAO>().save_new(db_rel.clone()).await {
        Err(e) => {
            let r = db_rel.into();
            log::error!("Storing running Yagna release {} to DB: {}", r, e);
            r
        }
        Ok(r) => {
            log::info!("Stored currently running Yagna release {} to DB", r);
            r
        }
    };
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::release_url;

    #[test]
    fn builds_latest_and_tagged_release_urls() {
        assert_eq!(
            release_url(None).unwrap().as_str(),
            "https://api.github.com/repos/golemfactory/yagna/releases/latest"
        );
        assert_eq!(
            release_url(Some("release/v0.17.6")).unwrap().as_str(),
            "https://api.github.com/repos/golemfactory/yagna/releases/tags/release%2Fv0.17.6"
        );
    }
}
