use anyhow::anyhow;

use crate::YagnaMock;

pub trait YagnaCli {
    fn appkey_list_json(&self) -> anyhow::Result<Vec<serde_json::Value>>;
}

impl YagnaCli for YagnaMock {
    fn appkey_list_json(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        let output = self
            .command()
            .arg("app-key")
            .arg("list")
            .arg("--json")
            .output()?;
        let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        result
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow!("`yagna app-key list --json` output is not a json array."))
    }
}
