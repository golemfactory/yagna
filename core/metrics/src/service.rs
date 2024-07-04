use actix_web::web::Path;
use futures::lock::Mutex;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Arc;
use structopt::StructOpt;
use url::Url;

use ya_service_api::{CliCtx, MetricsCtx};
use ya_service_api_interfaces::Provider;
use ya_utils_consent::ConsentType;

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
    #[structopt(flatten)]
    pub labels: MetricsLabels,
}

#[derive(structopt::StructOpt, Debug)]
pub struct MetricsLabels {
    /// Allows to create arbitrary group of Nodes that can be distinguished
    /// from the crowd.
    #[structopt(long, env = "YAGNA_METRICS_GROUP")]
    pub group: Option<String>,
}

impl From<&MetricsPusherOpts> for MetricsCtx {
    fn from(opts: &MetricsPusherOpts) -> Self {
        let mut labels = HashMap::new();
        if let Some(group) = &opts.labels.group {
            labels.insert("group".to_string(), group.to_string());
        }

        MetricsCtx {
            push_enabled: !opts.disable_metrics_push,
            push_host_url: Some(opts.metrics_push_url.clone()),
            job: opts.metrics_job_name.clone(),
            labels,
        }
    }
}

impl MetricsPusherOpts {
    pub fn from_env() -> Result<MetricsPusherOpts, structopt::clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        MetricsPusherOpts::from_iter_safe(&[""])
    }
}

pub struct MetricsService;

lazy_static! {
    static ref METRICS: Arc<Mutex<Metrics>> = Metrics::new();
}

pub async fn export_metrics_filtered_web(typ: Path<String>) -> String {
    let allowed_prefixes = typ.split(',').collect::<Vec<_>>();
    log::info!("Allowed prefixes: {:?}", allowed_prefixes);
    let filter = MetricsFilter {
        allowed_prefixes: &allowed_prefixes,
    };
    export_metrics_filtered(Some(filter)).await
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
            .route("/expose", actix_web::web::get().to(export_metrics_local))
            .route("/sorted", actix_web::web::get().to(export_metrics_sorted))
            .route(
                "/filtered/{typ}",
                actix_web::web::get().to(export_metrics_filtered_web),
            )
            .route(
                "/filtered",
                actix_web::web::get().to(export_metrics_for_push),
            )
    }
}

pub(crate) struct MetricsFilter<'a> {
    pub allowed_prefixes: &'a [&'a str],
}

//algorith is returning metrics in random order, which is fine for prometheus, but not for human checking metrics
pub fn sort_metrics_txt(metrics: &str, filter: Option<MetricsFilter<'_>>) -> String {
    let Some(first_line_idx) = metrics.find('\n') else {
        return metrics.to_string();
    };
    let (first_line, metrics_content) = metrics.split_at(first_line_idx);

    let entries = metrics_content
        .split("\n\n") //splitting by double new line to get separate metrics
        .map(|s| {
            let trimmed = s.trim();
            let mut lines = trimmed.split('\n').collect::<Vec<_>>();
            lines.sort(); //sort by properties
            (lines.get(1).unwrap_or(&"").to_string(), lines.join("\n"))
        })
        .collect::<Vec<(String, String)>>();

    let mut final_entries = if let Some(filter) = filter {
        let mut final_entries = Vec::with_capacity(entries.len());
        for entry in entries {
            for prefix in filter.allowed_prefixes {
                if entry.0.starts_with(prefix) {
                    log::info!("Adding entry: {}", entry.0);
                    final_entries.push(entry.1);
                    break;
                }
            }
        }
        final_entries
    } else {
        entries.into_iter().map(|(_, s)| s).collect()
    };

    final_entries.sort();

    first_line.to_string() + "\n" + final_entries.join("\n\n").as_str()
}

pub async fn export_metrics_filtered(metrics_filter: Option<MetricsFilter<'_>>) -> String {
    sort_metrics_txt(&METRICS.lock().await.export(), metrics_filter)
}

async fn export_metrics_sorted() -> String {
    sort_metrics_txt(&METRICS.lock().await.export(), None)
}
const ALLOWED_PREFIXES_EXTERNAL: &[&str] = &["market_agree", "erc20_pay"];
const ALLOWED_PREFIXES_INTERNAL: &[&str] = &["payment_invoices", "payment_debit"];

pub async fn export_metrics_for_push() -> String {
    let internal_consent =
        ya_utils_consent::have_consent_cached(ConsentType::Internal).unwrap_or(false);
    let external_consent =
        ya_utils_consent::have_consent_cached(ConsentType::External).unwrap_or(false);
    let filter = if internal_consent && external_consent {
        log::info!("Pushing all metrics, because both internal and external consents are given");
        None
    } else if internal_consent && !external_consent {
        log::info!("Pushing only internal metrics, because internal consent is given");
        Some(MetricsFilter {
            allowed_prefixes: ALLOWED_PREFIXES_INTERNAL,
        })
    } else if !internal_consent && external_consent {
        log::info!("Pushing only external metrics, because external consent is given");
        Some(MetricsFilter {
            allowed_prefixes: ALLOWED_PREFIXES_EXTERNAL,
        })
    } else {
        // !internal_consent && !external_consent
        log::info!(
            "Not pushing metrics, because both internal and external consents are not given"
        );
        return "".to_string();
    };

    export_metrics_filtered(filter).await
}

pub async fn export_metrics_local() -> String {
    export_metrics_sorted().await
}
