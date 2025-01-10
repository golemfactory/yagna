use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use url::Url;
use std::fs;

pub fn download_cert_if_url(cert_source: &PathBuf, temp_dir: &Path) -> Result<PathBuf> {
    let source_str = cert_source.to_string_lossy();
    if let Ok(url) = Url::parse(&source_str) {
        if url.scheme() == "http" || url.scheme() == "https" {
            download_cert(&source_str, temp_dir)
        } else {
            Ok(cert_source.clone())
        }
    } else {
        Ok(cert_source.clone())
    }
}

fn download_cert(url: &str, temp_dir: &Path) -> Result<PathBuf> {
    // Create temp directory if it doesn't exist
    fs::create_dir_all(temp_dir)?;

    // Generate a unique filename based on the URL
    let file_name = url
        .split('/')
        .last()
        .ok_or_else(|| anyhow!("Invalid URL format"))?;
    let temp_path = temp_dir.join(file_name);

    // Download the certificate
    let response = reqwest::blocking::get(url)?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to download certificate. Status: {}",
            response.status()
        ));
    }

    // Save the certificate to a temporary file
    let content = response.bytes()?;
    fs::write(&temp_path, content)?;

    Ok(temp_path)
}
