#![allow(dead_code)]

use anyhow::anyhow;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;

use super::identity::MockIdentity;
use super::payment::MockPayment;

/// Represents Node abstraction in tests.
/// Provides functionality to instantiate selected modules and make tests setup easier.
///
/// TODO: Currently setup with multiple Nodes with GSB bound modules is impossible, because
///       most yagna modules bind to fixed GSB addresses and have dependencies on other modules,
///       using fixed addresses. This should be improved in the future.
#[derive(Clone)]
pub struct MockNode {
    name: String,
    testdir: PathBuf,
    pub identity: Option<MockIdentity>,
    pub payment: Option<MockPayment>,
}

impl MockNode {
    pub fn new(name: &str, testdir: &Path) -> Self {
        let testdir = testdir.join(name);
        MockNode {
            name: name.to_string(),
            testdir,
            identity: None,
            payment: None,
        }
    }

    pub fn with_identity(mut self) -> Self {
        self.identity = Some(MockIdentity::new(&self.name));
        self
    }

    pub fn with_payment(mut self) -> Self {
        self.payment = Some(MockPayment::new(&self.name));
        self
    }

    pub fn get_identity(&self) -> anyhow::Result<MockIdentity> {
        self.identity
            .clone()
            .ok_or_else(|| anyhow!("Identity ({}) is not initialized", self.name))
    }

    pub fn get_payment(&self) -> anyhow::Result<MockPayment> {
        self.payment
            .clone()
            .ok_or_else(|| anyhow!("Payment ({}) is not initialized", self.name))
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        self.bind_gsb_router().await?;

        if let Some(identity) = &self.identity {
            identity.bind_gsb().await?;
        }

        if let Some(payment) = &self.payment {
            payment.bind_gsb().await?;
        }

        Ok(())
    }

    pub async fn bind_gsb_router(&self) -> anyhow::Result<()> {
        let gsb_url = self.gsb_router_address()?;

        log::info!(
            "MockeNode ({}) - binding GSB router at: {gsb_url}",
            self.name
        );

        ya_sb_router::bind_gsb_router(Some(gsb_url.clone()))
            .await
            .map_err(|e| anyhow!("Error binding service bus router to '{}': {e}", &gsb_url))?;
        Ok(())
    }

    fn gsb_router_address(&self) -> anyhow::Result<Url> {
        let gsb_url = match std::env::consts::FAMILY {
            // It would be better to create socket in self.testdir, but it's not possible, because
            // unix socket path length is limited to SUN_LEN (108 bytes).
            "unix" => Url::from_str(&format!("unix:///tmp/{}/gsb.sock", self.name))?,
            _ => Url::from_str(&format!(
                "tcp://127.0.0.1:{}",
                portpicker::pick_unused_port().ok_or(anyhow!("No ports free"))?
            ))?,
        };
        if gsb_url.scheme() == "unix" {
            let dir = PathBuf::from_str(gsb_url.path())?
                .parent()
                .map(|path| path.to_path_buf())
                .ok_or(anyhow!("`gsb_url` unix socket has no parent directory."))?;
            fs::create_dir_all(dir)?;
        }

        Ok(gsb_url)
    }
}
