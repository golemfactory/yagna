use crate::error::Error;
use crate::{abortable_sink, abortable_stream};
use crate::{TransferData, TransferProvider, TransferSink, TransferStream};
use bytes::Bytes;
use futures::future::ready;
use futures::{SinkExt, StreamExt, TryFutureExt, TryStreamExt};
use gftp::DEFAULT_CHUNK_SIZE;
use sha3::{Digest, Sha3_256};
use std::cmp::min;
use url::Url;
use ya_client_model::activity::TransferArgs;
use ya_core_model::gftp as model;
use ya_core_model::gftp::Error as GftpError;
use ya_core_model::gftp::GftpChunk;
use ya_net::TryRemoteEndpoint;
use ya_service_bus::RpcEndpoint;

pub struct GftpTransferProvider {
    rx_buffer_sz: usize,
}

impl Default for GftpTransferProvider {
    fn default() -> Self {
        GftpTransferProvider { rx_buffer_sz: 12 }
    }
}

impl TransferProvider<TransferData, Error> for GftpTransferProvider {
    fn schemes(&self) -> Vec<&'static str> {
        vec!["gftp"]
    }

    fn source(&self, url: &Url, _: &TransferArgs) -> TransferStream<TransferData, Error> {
        let url = url.clone();
        let buffer_sz = self.rx_buffer_sz;
        let chunk_size = DEFAULT_CHUNK_SIZE;

        let (stream, tx, abort_reg) = TransferStream::<TransferData, Error>::create(1);
        let txc = tx.clone();

        tokio::task::spawn_local(async move {
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
                    .buffered(buffer_sz)
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

    fn destination(&self, url: &Url, _: &TransferArgs) -> TransferSink<TransferData, Error> {
        let url = url.clone();
        let chunk_size = DEFAULT_CHUNK_SIZE as usize;

        let (sink, mut rx, res_tx) = TransferSink::<TransferData, Error>::create(1);

        tokio::task::spawn_local(async move {
            let fut = async move {
                let (node_id, random_filename) = gftp::extract_url(&url)
                    .map_err(|_| Error::InvalidUrlError("Invalid gftp URL".to_owned()))?;
                let remote = node_id.try_service(&model::file_bus_id(&random_filename))?;

                let mut digest = Sha3_256::default();
                let mut offset: usize = 0;
                while let Some(result) = rx.next().await {
                    let bytes = Bytes::from(result?);
                    let n = (bytes.len() + chunk_size - 1) / chunk_size;
                    for i in 0..n {
                        let start = i * chunk_size;
                        let end = start + min(bytes.len() - start, chunk_size);
                        let chunk = GftpChunk {
                            offset: offset as u64,
                            content: bytes[start..end].to_vec(),
                        };
                        offset += chunk.content.len();
                        digest.input(&chunk.content);
                        remote.call(model::UploadChunk { chunk }).await??;
                    }
                }

                let hash = Some(format!("{:x}", digest.result()));
                remote.call(model::UploadFinished { hash }).await??;
                Result::<(), Error>::Ok(())
            }
            .map_err(Error::from);

            abortable_sink(fut, res_tx).await
        });

        sink
    }
}
