use anyhow::Context;
use std::{
    fmt::{Display, Error, Formatter},
    ops::Not,
    path::PathBuf,
    str::FromStr,
};

pub const DEFAULT_ORG_NAME: &str = "GolemFactory";

#[derive(Debug, PartialEq)]
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
            if self != &Self::default() {
                anyhow::bail!(format!("given data dir {} does not exist", self))
            } else {
                log::info!("creating default data dir: {}", self);
                Ok(std::fs::create_dir_all(&self.0)
                    .context(format!("default data dir {} creation error", self))
                    .map(|_| self.0.to_owned())?)
            }
        } else {
            Ok(self.0.to_owned())
        }
    }
}
