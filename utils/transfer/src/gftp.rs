use crate::error::Error;
use crate::{abortable_sink, abortable_stream};
use crate::{TransferContext, TransferData, TransferProvider, TransferSink, TransferStream};
use bytes::Bytes;
use futures::channel::mpsc;
use futures::future::{ready, try_select, Either};
use futures::{FutureExt, SinkExt, StreamExt, TryFutureExt, TryStreamExt};
use gftp::DEFAULT_CHUNK_SIZE;
use sha3::{Digest, Sha3_256};
use tokio::task::spawn_local;
use url::Url;
use ya_core_model::gftp as model;
use ya_core_model::gftp::Error as GftpError;
use ya_core_model::gftp::GftpChunk;
use ya_net::TryRemoteEndpoint;
use ya_service_bus::RpcEndpoint;

pub struct GftpTransferProvider {
    concurrency: usize,
}

impl Default for GftpTransferProvider {
    fn default() -> Self {
        GftpTransferProvider { concurrency: 8 }
    }
}

impl TransferProvider<TransferData, Error> for GftpTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["gftp"]
    }

    fn source(&self, url: &Url, _: &TransferContext) -> TransferStream<TransferData, Error> {
        let url = url.clone();
        let concurrency = self.concurrency;
        let chunk_size = DEFAULT_CHUNK_SIZE;

        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let txc = tx.clone();

        spawn_local(async move {
            let fut = async move {
                let (node_id, hash) = gftp::extract_url(&url)
                    .map_err(|_| Error::InvalidUrlError("Invalid gftp URL".to_owned()))?;

                let remote = node_id.try_service(&model::file_bus_id(&hash))?;
                let meta = remote.send(model::GetMetadata {}).await??;
                let n = (meta.file_size + chunk_size - 1) / chunk_size;

                futures::stream::iter(0..n)
                    .map(|chunk_number| {
                        remote.call(model::GetChunk {
                            offset: chunk_number * chunk_size,
                            size: chunk_size,
                        })
                    })
                    .buffered(concurrency)
                    .map_err(Error::from)
                    .forward(tx.sink_map_err(Error::from).with(
                        |r: Result<GftpChunk, GftpError>| {
                            ready(Ok(match r {
                                Ok(c) => Ok(TransferData::from(Into::<Bytes>::into(c.content))),
                                Err(e) => Err(Error::from(e)),
                            }))
                        },
                    ))
                    .await
                    .map_err(Error::from)
            };

            abortable_stream(fut, abort_reg, txc).await
        });

        stream
    }

    fn destination(&self, url: &Url, _: &TransferContext) -> TransferSink<TransferData, Error> {
        let url = url.clone();
        let concurrency = self.concurrency;
        let chunk_size = DEFAULT_CHUNK_SIZE as usize;

        let (sink, mut rx, res_tx) = TransferSink::<TransferData, Error>::create(1);
        let (mut chunk_tx, chunk_rx) = mpsc::channel(concurrency);
        let mut chunk_txc = chunk_tx.clone();

        let mut offset = 0;

        spawn_local(async move {
            let fut = async move {
                let (node_id, random_filename) = gftp::extract_url(&url)
                    .map_err(|_| Error::InvalidUrlError("invalid gftp URL".into()))?;
                let remote = node_id.try_service(&model::file_bus_id(&random_filename))?;

                let digest_fut = async move {
                    let mut digest = Sha3_256::default();

                    while let Some(result) = rx.next().await {
                        let bytes = Bytes::from(result?);
                        let n = (bytes.len() + chunk_size - 1) / chunk_size;

                        for i in 0..n {
                            let start = i * chunk_size;
                            let end = start + chunk_size.min(bytes.len() - start);
                            let chunk = GftpChunk {
                                offset: offset as u64,
                                content: bytes[start..end].to_vec(),
                            };

                            offset += chunk.content.len();
                            digest.input(&chunk.content);
                            chunk_tx.send(Ok::<_, Error>(chunk)).await?;
                        }
                    }

                    Ok::<_, Error>(digest.result())
                };

                let send_fut = chunk_rx.try_for_each_concurrent(concurrency, |chunk| async {
                    remote.call(model::UploadChunk { chunk }).await??;
                    Ok(())
                });

                let result = try_select(digest_fut.boxed_local(), send_fut.boxed_local()).await;
                let _ = chunk_txc.flush().await;
                chunk_txc.close().await?;

                let digest = match result {
                    Ok(Either::Left((d, f))) => f.await.map(|_| d)?,
                    Ok(Either::Right((_, f))) => f.await?,
                    Err(either) => return Err(either.factor_first().0),
                };

                let hash = Some(format!("{:x}", digest));
                remote.call(model::UploadFinished { hash }).await??;
                Result::<(), Error>::Ok(())
            }
            .map_err(Error::from);

            abortable_sink(fut, res_tx).await
        });

        sink
    }
}
