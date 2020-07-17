use crate::normalize_path;
use anyhow::Context;
use std::{
    fmt::{Display, Error, Formatter},
    ops::Not,
    path::PathBuf,
    str::FromStr,
};

pub const DEFAULT_ORG_NAME: &str = "GolemFactory";

#[derive(Clone, Debug, PartialEq)]
pub struct DataDir(PathBuf);

impl DataDir {
    pub fn new(organisation: &str, app_name: &str) -> Self {
        DataDir(
            directories::ProjectDirs::from("", organisation, app_name)
                .map(|dirs| dirs.data_dir().into())
                .unwrap_or_else(|| PathBuf::from(DEFAULT_ORG_NAME)),
        )
    }
}

impl Default for DataDir {
    #[inline]
    fn default() -> Self {
        Self::new(DEFAULT_ORG_NAME, "")
    }
}

impl FromStr for DataDir {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(DataDir(PathBuf::from(s.trim_matches('"'))))
    }
}

impl Display for DataDir {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "{:?}", self.0)
    }
}

impl DataDir {
    pub fn get_or_create(&self) -> anyhow::Result<PathBuf> {
        if self.0.exists().not() {
            log::info!("creating data dir: {}", self);
            std::fs::create_dir_all(&self.0)
                .context(format!("data dir {} creation error", self))
                .map(|_| self.0.to_owned())?;
        }
        Ok(normalize_path(&self.0)?)
    }
}
