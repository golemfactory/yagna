use std::{collections::HashSet, path::PathBuf};

use super::Keystore;

pub(super) trait GolemCertAddParams {}

#[derive(Debug)]
struct GolemKeystore {
    cert_files: HashSet<String, PathBuf>,
    cert_dir: PathBuf,
}

impl Keystore for GolemKeystore {
    fn load(_cert_dir: &PathBuf) -> anyhow::Result<Self>
    {
        todo!()
    }

    fn reload(&mut self, _cert_dir: &PathBuf) -> anyhow::Result<()> {
        todo!()
    }

    fn add(&mut self, _add: &super::AddParams) -> anyhow::Result<super::AddResponse> {
        Ok(Default::default())
    }

    fn remove(&mut self, _remove: &super::RemoveParams) -> anyhow::Result<super::RemoveResponse> {
        Ok(Default::default())
    }

    fn list(&self) -> Vec<super::Cert> {
        Default::default()
    }
}
