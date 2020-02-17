use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::fs::OpenOptions;


#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    /// Deployment id in url like form.
    pub id: String,
    pub name: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mount_points: Vec<MountPoint>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum MountPoint {
    Ro(String),
    Rw(String),
    Wo(String),
}

impl MountPoint {
    pub fn path(&self) -> &str {
        match self {
            MountPoint::Ro(path) => path,
            MountPoint::Rw(path) => path,
            MountPoint::Wo(path) => path,
        }
    }
}


pub fn load_manifest(image_path: &Path) -> Result<Manifest> {
    let mut archive = zip::ZipArchive::new(OpenOptions::new().read(true).open(image_path)?)?;
    let entry = archive.by_name("manifest.json")?;

    Ok(serde_json::from_reader(entry)?)
}
