use std::collections::HashMap;
use std::path::PathBuf;

use ya_core_model::bus::GsbBindPoints;
pub use ya_utils_cli::{CommandOutput, ResponseTable};

#[derive(Clone, Debug, Default)]
pub struct MetricsCtx {
    pub push_enabled: bool,
    pub push_host_url: Option<url::Url>,
    pub job: String,
    pub labels: HashMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct CliCtx {
    pub data_dir: PathBuf,
    pub gsb_url: Option<url::Url>,
    pub json_output: bool,
    pub accept_terms: bool,
    pub quiet: bool,
    pub metrics_ctx: Option<MetricsCtx>,
    pub gsb: GsbBindPoints,
}

impl CliCtx {
    pub fn output(&self, output: CommandOutput) -> Result<(), anyhow::Error> {
        output.print(self.json_output)
    }

    pub fn with_prefixed_gsb(mut self, gsb: Option<GsbBindPoints>) -> Self {
        if let Some(gsb) = gsb {
            self.gsb = gsb;
        }
        self
    }
}
