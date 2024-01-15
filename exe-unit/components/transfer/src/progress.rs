use crate::error::Error;
use crate::{abortable_sink, abortable_stream, TransferSink, TransferStream};
use crate::{TransferContext, TransferData};
use futures::{SinkExt, StreamExt, TryFutureExt};
use tokio::task::spawn_local;

type Stream = TransferStream<TransferData, Error>;

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
