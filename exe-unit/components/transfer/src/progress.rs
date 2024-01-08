use crate::error::Error;
use crate::{abortable_sink, abortable_stream, TransferSink, TransferStream};
use crate::{TransferContext, TransferData};

use futures::{SinkExt, StreamExt, TryFutureExt};
use std::sync::Arc;
use tokio::task::spawn_local;
use tokio::time::Instant;

use ya_client_model::activity::exe_script_command::ProgressArgs;
use ya_client_model::activity::CommandProgress;

type Stream = TransferStream<TransferData, Error>;

#[derive(Debug, Clone)]
pub struct ProgressConfig {
    /// Channel for watching for transfer progress.
    pub progress: tokio::sync::broadcast::Sender<CommandProgress>,
    pub progress_args: ProgressArgs,
}

#[derive(Default, Clone)]
pub struct ProgressReporter {
    config: ProgressArgs,
    inner: Arc<std::sync::Mutex<Option<ProgressImpl>>>,
}

struct ProgressImpl {
    pub report: tokio::sync::broadcast::Sender<CommandProgress>,
    pub last: CommandProgress,
    pub last_update: Instant,
}

impl ProgressReporter {
    pub fn next_step(&self) {
        self.inner.lock().unwrap().as_mut().map(|inner| {
            inner.last.step.0 += 1;
            inner.last.progress = (0, None);
            inner.last_update = Instant::now();
            inner
                .report
                .send(CommandProgress {
                    message: None,
                    ..inner.last.clone()
                })
                .ok()
        });
    }

    /// TODO: implement `update_interval` and `step`
    pub fn report_progress(&self, progress: u64, size: Option<u64>) {
        let _update_interval = self.config.update_interval;
        let _step = self.config.step;

        self.inner.lock().unwrap().as_mut().map(|inner| {
            inner.last.progress = (progress, size);
            inner.last_update = Instant::now();
            inner
                .report
                .send(CommandProgress {
                    message: None,
                    ..inner.last.clone()
                })
                .ok()
        });
    }

    pub fn report_message(&self, message: String) {
        self.inner.lock().unwrap().as_mut().map(|inner| {
            inner.last_update = Instant::now();
            inner
                .report
                .send(CommandProgress {
                    message: Some(message),
                    ..inner.last.clone()
                })
                .ok()
        });
    }

    pub fn register_reporter(
        &self,
        args: Option<ProgressConfig>,
        steps: usize,
        unit: Option<String>,
    ) {
        if let Some(args) = args {
            *(self.inner.lock().unwrap()) = Some(ProgressImpl {
                report: args.progress,
                last: CommandProgress {
                    step: (0, steps),
                    message: None,
                    progress: (0, None),
                    unit,
                },
                last_update: Instant::now(),
            });
        }
    }
}

/// Wraps a stream to report progress.
/// The `report` function is called with the current offset and the total size.
/// The total size is 0 if the size is unknown. (For example, when the source is a directory.)
/// The reporting function should not block as it will block the transfer.
pub fn wrap_stream_with_progress_reporting<F>(
    mut source: Stream,
    ctx: &TransferContext,
    report: F,
) -> Stream
where
    F: Fn(u64, Option<u64>) + Send + 'static,
{
    let (stream, tx, abort_reg) = Stream::create(0);
    let mut txc = tx.clone();
    let state = ctx.state.clone();

    spawn_local(async move {
        let fut = async move {
            let mut offset = state.offset();
            while let Some(result) = source.next().await {
                if let Ok(data) = result.as_ref() {
                    let data_len = data.as_ref().len() as u64;
                    offset += data_len;
                    report(offset, state.size());
                }
                txc.send(result).await?;
            }
            Ok::<(), Error>(())
        }
        .map_err(|error| {
            log::error!("Error forwarding data: {}", error);
            error
        });

        abortable_stream(fut, abort_reg, tx).await
    });

    stream
}

type Sink = TransferSink<TransferData, Error>;

pub fn progress_report_channel(dest: Sink, ctx: &TransferContext) -> Sink {
    let report = ctx.reporter();
    wrap_sink_with_progress_reporting(dest, ctx, move |progress, size| {
        report.report_progress(progress, size)
    })
}

/// Wraps a sink to report progress.
/// The `report` function is called with the current offset and the total size.
/// The total size is 0 if the size is unknown. (For example, when the source is a directory.)
/// The reporting function should not block as it will block the transfer.
pub fn wrap_sink_with_progress_reporting<F>(
    mut dest: Sink,
    ctx: &TransferContext,
    report: F,
) -> Sink
where
    F: Fn(u64, Option<u64>) + Send + 'static,
{
    let (mut sink, mut rx, res_tx) = Sink::create(0);
    let state = ctx.state.clone();
    sink.res_rx = dest.res_rx.take();

    spawn_local(async move {
        let fut = async move {
            let mut offset = state.offset();
            while let Some(result) = rx.next().await {
                let data = result?;
                let data_len = data.as_ref().len() as u64;
                offset += data_len;
                report(offset, state.size());

                dest.send(data).await?;
                if data_len == 0 {
                    break;
                }
            }

            Ok::<(), Error>(())
        }
        .map_err(|error| {
            log::error!("Error forwarding data: {}", error);
            error
        });

        abortable_sink(fut, res_tx).await
    });

    sink
}
