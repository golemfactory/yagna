use crate::commands::{MetricReportReq, RegisterService, Shutdown, ShutdownReason};
use crate::error::{Error, LocalServiceError};
use crate::metrics::{CpuMetric, MemMetric, Metric, MetricData, MetricReport};
use crate::runtime::Runtime;
use crate::service::metrics::MetricService;
use crate::service::reporter::Reporter;
use crate::service::signal::SignalMonitor;
use crate::{ExeUnit, Result};
use actix::{Actor, Addr};
use futures::Future;
use std::pin::Pin;
use std::time::Duration;
use ya_core_model::activity::SetActivityUsage;
use ya_model::activity::ActivityUsage;

lazy_static::lazy_static! {
    static ref DEFAULT_INTERVAL: Duration = Duration::from_secs(1u64);
}

pub async fn register<R: Runtime>(
    addr: Addr<ExeUnit<R>>,
    activity_api_id: impl ToString,
    activity_id: impl ToString,
) -> Result<()> {
    let signal = SignalMonitor::new(addr.clone());
    let signal_actor = signal.start();
    let cpu_metrics = MetricService::new(CpuMetric::default());
    let cpu_actor = cpu_metrics.start();
    let mem_metrics = MetricService::new(MemMetric::default());
    let mem_actor = mem_metrics.start();

    let activity_id_ = activity_id.to_string();
    let addr_ = addr.clone();
    let cpu_actor_ = cpu_actor.clone();
    let mem_actor_ = mem_actor.clone();
    let usage_reporter = Reporter::new(activity_api_id, *DEFAULT_INTERVAL, move || {
        gather_metric_frames(
            activity_id_.clone(),
            addr_.clone(),
            cpu_actor_.clone(),
            mem_actor_.clone(),
        )
    });
    let usage_actor = usage_reporter.start();

    addr.send(RegisterService(signal_actor)).await?;
    addr.send(RegisterService(cpu_actor)).await?;
    addr.send(RegisterService(mem_actor)).await?;
    addr.send(RegisterService(usage_actor)).await?;

    Ok(())
}

macro_rules! parse_report {
    ($exe_unit:expr, $metric:expr, $report:expr) => {
        match $report {
            MetricReport::Frame(data) => Ok(data.as_f64()),
            MetricReport::Error(error) => Err(LocalServiceError::MetricError(error).into()),
            MetricReport::LimitExceeded(data) => {
                let msg = format!("{:?} usage exceeded: {:?}", $metric, data.as_f64());
                let shutdown = Shutdown(ShutdownReason::UsageLimitExceeded(msg.clone()));
                $exe_unit.send(shutdown).await??;
                Err(Error::UsageLimitExceeded(msg))
            }
        }
    };
}

pub fn gather_metric_frames<'a, R: Runtime>(
    activity_id: String,
    exe_unit: Addr<ExeUnit<R>>,
    cpu_actor: Addr<MetricService<CpuMetric>>,
    mem_actor: Addr<MetricService<MemMetric>>,
) -> Pin<Box<dyn Future<Output = Result<SetActivityUsage>> + 'a>> {
    Box::pin(async move {
        let cpu_report = cpu_actor.send(MetricReportReq::new()).await?;
        let cpu_data: f64 = parse_report!(exe_unit, CpuMetric::ID, cpu_report.0)?;
        let mem_report = mem_actor.send(MetricReportReq::new()).await?;
        let mem_data: f64 = parse_report!(exe_unit, MemMetric::ID, mem_report.0)?;

        Ok(SetActivityUsage {
            activity_id,
            timeout: None,
            usage: ActivityUsage {
                current_usage: Some(vec![cpu_data, mem_data]),
            },
        })
    })
}
