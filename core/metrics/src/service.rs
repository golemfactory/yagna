use futures::lock::Mutex;
use lazy_static::lazy_static;
use std::sync::Arc;
use url::Url;

use ya_service_api::{CliCtx, MetricsCtx};
use ya_service_api_interfaces::Provider;

use crate::metrics::Metrics;

const YAGNA_METRICS_URL_ENV_VAR: &str = "YAGNA_METRICS_URL";
const DEFAULT_YAGNA_METRICS_URL: &str = "https://metrics.golem.network:9092/";

// TODO: enable showing metrics also via CLI
#[derive(structopt::StructOpt, Debug)]
pub struct MetricsPusherOpts {
    /// Disable metrics pushing
    #[structopt(long)]
    pub disable_metrics_push: bool,

    /// Metrics push host url
    #[structopt(
        long,
        env = YAGNA_METRICS_URL_ENV_VAR,
        default_value = DEFAULT_YAGNA_METRICS_URL,
    )]
    pub metrics_push_url: Url,
    /// Metrics job name, which allows to distinguish different groups of Nodes.
    #[structopt(
        long,
        env = "YAGNA_METRICS_JOB_NAME",
        default_value = "community.hybrid"
    )]
    pub metrics_job_name: String,
}

impl From<&MetricsPusherOpts> for MetricsCtx {
    fn from(opts: &MetricsPusherOpts) -> Self {
        MetricsCtx {
            push_enabled: !opts.disable_metrics_push,
            push_host_url: Some(opts.metrics_push_url.clone()),
            job: opts.metrics_job_name.clone(),
        }
    }
}

pub struct MetricsService;

lazy_static! {
    static ref METRICS: Arc<Mutex<Metrics>> = Metrics::new();
}

impl MetricsService {
    pub async fn gsb<C: Provider<Self, CliCtx>>(context: &C) -> anyhow::Result<()> {
        // This should initialize Metrics. We need to do this before all other services will start.
        let _ = METRICS.clone();

        crate::pusher::spawn(
            context
                .component()
                .metrics_ctx
                .expect("Metrics pusher needs CLI ctx"),
        );
        Ok(())
    }

    pub fn rest<C: Provider<Self, ()>>(_ctx: &C) -> actix_web::Scope {
        actix_web::Scope::new("metrics-api/v1")
            // TODO:: add wrapper injecting Bearer to avoid hack in auth middleware
            .route("/expose", actix_web::web::get().to(export_metrics))
    }
}

pub async fn export_metrics() -> String {
    METRICS.lock().await.export()
}
