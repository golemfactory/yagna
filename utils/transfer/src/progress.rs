use crate::error::Error;
use crate::{TransferStream, abortable_stream};
use crate::{TransferContext, TransferData};
use futures::{SinkExt, StreamExt, TryFutureExt};
use tokio::task::spawn_local;

type Stream = TransferStream<TransferData, Error>;

/// Wraps a stream to report progress.
/// The `report` function is called with the current offset and the total size.
/// The total size is 0 if the size is unknown. (For example, when the source is a directory.)
pub fn wrap_with_progress_reporting<F>(mut source: Stream, ctx: &TransferContext, report: F) -> Stream
where
    F: Fn(u64, u64) -> () + Send + 'static,
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
                    offset += data_len as u64;
                    report(offset, state.size().unwrap_or(0));
                }
                txc.send(result).await?;
            };
            Ok::<(), Error>(())
        }
        .map_err(|error| {
            log::error!("Error forwading data: {}", error);
            error
        });

        abortable_stream(fut, abort_reg, tx).await
    });

    stream
}
