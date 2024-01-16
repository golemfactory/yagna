use crate::error::Error;
use crate::{abortable_sink, abortable_stream, TransferSink, TransferStream};
use crate::{TransferContext, TransferData};

use futures::{SinkExt, StreamExt, TryFutureExt};
use std::sync::Arc;
use std::time::Duration;
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
    pub last_send: Instant,
}

impl ProgressReporter {
    pub fn next_step(&self) {
        self.inner.lock().unwrap().as_mut().map(|inner| {
            inner.last.step.0 += 1;
            inner.last.progress = (0, None);
            inner.last_send = Instant::now();
            inner
                .report
                .send(CommandProgress {
                    message: None,
                    ..inner.last.clone()
                })
                .ok()
        });
    }

    /// TODO: implement `update_step`
    pub fn report_progress(&self, progress: u64, size: Option<u64>) {
        let update_interval: Duration = self
            .config
            .update_interval
            .map(Into::into)
            .unwrap_or(Duration::from_secs(1));
        let _update_step = self.config.update_step;

        self.inner.lock().unwrap().as_mut().map(|inner| {
            inner.last.progress = (progress, size);
            if inner.last_send + update_interval <= Instant::now() {
                inner.last_send = Instant::now();
                inner
                    .report
                    .send(CommandProgress {
                        message: None,
                        ..inner.last.clone()
                    })
                    .ok();
            }
        });
    }

    pub fn report_message(&self, message: String) {
        self.inner.lock().unwrap().as_mut().map(|inner| {
            inner.last_send = Instant::now();
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
        &mut self,
        args: Option<ProgressConfig>,
        steps: usize,
        unit: Option<String>,
    ) {
        if let Some(args) = args {
            self.config = args.progress_args;
            *(self.inner.lock().unwrap()) = Some(ProgressImpl {
                report: args.progress,
                last: CommandProgress {
                    step: (0, steps),
                    message: None,
                    progress: (0, None),
                    unit,
                },
                last_send: Instant::now(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    use duration_string::DurationString;
    use tokio::time::Duration;

    #[actix_rt::test]
    async fn test_progress_reporter_interval() {
        let mut report = ProgressReporter::default();
        let (tx, mut rx) = tokio::sync::broadcast::channel(10);
        report.register_reporter(
            Some(ProgressConfig {
                progress: tx,
                progress_args: ProgressArgs {
                    update_interval: Some("500ms".parse::<DurationString>().unwrap()),
                    update_step: None,
                },
            }),
            2,
            Some("Bytes".to_string()),
        );

        let size = 200;
        let mut before = Instant::now();
        tokio::task::spawn_local(async move {
            for step in 0..2 {
                tokio::time::sleep(Duration::from_millis(25)).await;

                for i in 0..=size {
                    report.report_progress(i, Some(size));
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                if step == 0 {
                    report.next_step();
                }
            }
            report.report_message("Finished".to_string());
        });

        let mut counter = 0;
        let mut step = 0;
        while let Ok(event) = rx.recv().await {
            //println!("{event:?}");

            counter += 1;
            let update = Instant::now().duration_since(before);
            before = Instant::now();
            let diff = if update > Duration::from_millis(525) {
                update - Duration::from_millis(525)
            } else {
                Duration::from_millis(525) - update
            };

            assert!(diff <= Duration::from_millis(20));

            // `ProgressReporter` should ignore 10 messages in each loop.
            assert_eq!(event.progress.0, counter * 10);
            assert_eq!(event.progress.1, Some(size));
            assert_eq!(event.step, (step, 2));
            assert_eq!(event.unit, Some("Bytes".to_string()));
            assert_eq!(event.message, None);

            if counter == 20 {
                if step == 1 {
                    break;
                }

                counter = 0;
                step += 1;

                // Skip step change event
                rx.recv().await.unwrap();
                before = Instant::now();
            }
        }

        // Reporting message will result in event containing progress adn step from previous event.
        let last = rx.recv().await.unwrap();
        //println!("{last:?}");
        assert_eq!(last.message, Some("Finished".to_string()));
        assert_eq!(last.progress.0, size);
        assert_eq!(last.progress.1, Some(size));
        assert_eq!(last.step, (1, 2));
    }
}
