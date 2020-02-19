use anyhow::{Result, Context};
use reqwest;
use crypto::sha3::{Sha3, Sha3Mode};
use std::path::{Path, PathBuf};
use crypto::digest::Digest;
use std::fs::File;
use std::io;


pub fn download_image_http(url: &str, cachedir: &Path) -> Result<PathBuf> {
    let mut response = reqwest::blocking::get(url)
        .with_context(|| format!("Can't download image from url {}.", url))?;

    let image_file_path = cachedir.join(url_to_filename(url));
    let mut image_file = File::create(&image_file_path)
        .with_context(|| format!("Can't create image file {}.", image_file_path.display()))?;

    io::copy(&mut response, &mut image_file)
        .with_context(|| format!("Can't copy downloaded file to destination {}.", image_file_path.display()))?;
    Ok(image_file_path)
}

fn url_to_filename(url: &str) -> String {
    let mut hasher = Sha3::new(Sha3Mode::Sha3_512);
    hasher.input_str(url);
    hasher.result_str()
}

