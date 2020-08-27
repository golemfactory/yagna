use anyhow::Result;
use sha3::{Digest, Sha3_512};
use std::path::PathBuf;
use tokio::fs;
use url::Url;

/// Represents a path/url to a Yagna package.
#[derive(Debug, Clone)]
pub enum Package {
    /// Path to Yagna package. Hash information will be computed automatically.
    Archive(PathBuf),
    /// URL to a resource already published using `gftp` protocol.
    ///
    /// # Example:
    /// ```rust
    /// use ya_requestor_sdk::Package;
    /// let package = Package::Url { digest: "beefdead".to_string(), url: "gftp:deadbeef/deadbeef".to_string() };
    /// ```
    Url { digest: String, url: String },
}

impl Package {
    /// Publishes the `Package` if specified as `Package::Archive`, and computes
    /// the package's `sha3` hash.
    ///
    /// If the `Package` is specified as `Package::Url`, verifies the url is correct
    /// but does not re-publish the package (assumes it is already published).
    ///
    /// In all cases, `gftp` is the assumed communication medium.
    pub async fn publish(&self) -> Result<(String, Url)> {
        match self {
            Self::Archive(path) => {
                let image_path = path
                    .canonicalize()
                    .map_err(|e| anyhow::anyhow!("invalid image path {:?}: {}", path, e))?;

                log::info!("image file path: {}", image_path.display());

                let url = gftp::publish(&path).await.map_err(|e| {
                    anyhow::anyhow!("gftp: unable to publish image {:?}: {}", path, e)
                })?;

                log::info!("image published at: {}", url);

                let contents = fs::read(&image_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("unable to open image {:?}: {}", image_path, e))?;
                let digest = Sha3_512::digest(&contents);
                let digest = format!("{:x}", digest);

                log::info!("image's computed digest: {}", digest);

                Ok((digest, url))
            }
            Self::Url { digest, url } => {
                let url = Url::parse(&url)
                    .map_err(|e| anyhow::anyhow!("invalid URL \"{}\": {}", url, e))?;

                log::info!("parsed url for image file: {}", url);
                log::info!("digest of the published image: {}", digest);

                Ok((digest.clone(), url))
            }
        }
    }
}

#[derive(Clone)]
pub enum Image {
    WebAssembly(semver::Version),
    GVMKit(semver::Version),
    Sgx(semver::Version),
}

impl Image {
    pub fn runtime_name(&self) -> &'static str {
        match self {
            Image::WebAssembly(_) => "wasmtime",
            Image::GVMKit(_) => "vm",
            Image::Sgx(_) => "sgx",
        }
    }

    pub fn runtime_version(&self) -> semver::Version {
        match self {
            Image::WebAssembly(version) | Image::GVMKit(version) | Image::Sgx(version) => {
                version.clone()
            }
        }
    }
}
