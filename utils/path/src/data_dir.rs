use crate::normalize_path;
use anyhow::Context;
use std::fmt::Display;
use std::{ops::Not, path::PathBuf, str::FromStr};

const ORGANIZATION: &str = "GolemFactory";
const QUALIFIER: &str = "";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDir(PathBuf);

impl DataDir {
    pub fn new(app_name: &str) -> Self {
        DataDir(
            directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, app_name)
                .map(|dirs| dirs.data_dir().into())
                .unwrap_or_else(|| PathBuf::from(ORGANIZATION).join(app_name)),
        )
    }

    pub fn get_or_create(&self) -> anyhow::Result<PathBuf> {
        if self.0.exists().not() {
            // not using logger here bc it might haven't been initialized yet
            eprintln!("Creating data dir: {}", self.0.display());
            std::fs::create_dir_all(&self.0)
                .context(format!("data dir {:?} creation error", self))?;
        }
        Ok(normalize_path(&self.0)?)
    }
}

impl FromStr for DataDir {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(DataDir(PathBuf::from(s.trim_matches('"'))))
    }
}

impl Display for DataDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}
