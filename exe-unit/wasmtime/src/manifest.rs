use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs::{OpenOptions, File};
use zip::ZipArchive;
use std::io::Read;


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

pub struct WasmImage {
    archive: ZipArchive<File>,
    image_path: PathBuf,
    manifest: Manifest,
}


impl WasmImage {

    pub fn new(image_path: &Path) -> Result<WasmImage> {
        let mut archive = zip::ZipArchive::new(OpenOptions::new().read(true).open(image_path)?)?;
        let manifest = WasmImage::load_manifest(&mut archive)?;

        Ok(WasmImage{image_path: image_path.to_owned(), archive, manifest})
    }

    fn load_manifest(archive: &mut ZipArchive<File>) -> Result<Manifest> {
        let entry = archive.by_name("manifest.json")?;
        Ok(serde_json::from_reader(entry)?)
    }

    pub fn get_manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn get_module_name(&self) -> &str {
        &self.manifest.name
    }

    pub fn load_binary(&mut self) -> Result<Vec<u8>> {
        let binary_name = format!("{}.wasm", self.manifest.name);
        let mut entry = self.archive.by_name(&binary_name)?;

        let mut bytes = vec![];
        entry.read_to_end(&mut bytes)?;
        return Ok(bytes);
    }

    pub fn path(&self) -> &Path {
        return &self.image_path;
    }
}

