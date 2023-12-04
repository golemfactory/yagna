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
    #[structopt(long, env = "YAGNA_METRICS_JOB_NAME", default_value = "community.1")]
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
            .route("/sorted", actix_web::web::get().to(export_metrics_sorted))
    }
}
//algorith is returning metrics in random order, which is fine for prometheus, but not for human checking metrics
pub fn sort_metrics_txt(metrics: &str) -> String {
    let Some(first_line_idx) = metrics.find('\n') else {
        return metrics.to_string();
    };
    let (first_line, metrics_content) = metrics.split_at(first_line_idx);

    let mut entries = metrics_content
        .split("\n\n")
        .map(|s| s.trim().to_string())
        .collect::<Vec<String>>();
    entries.sort();

    first_line.to_string() + "\n" + entries.join("\n\n").as_str()
}

async fn export_metrics_sorted() -> String {
    sort_metrics_txt(&METRICS.lock().await.export())
}

pub async fn export_metrics() -> String {
    METRICS.lock().await.export()
}
