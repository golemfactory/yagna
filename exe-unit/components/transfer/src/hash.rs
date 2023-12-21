use actix::dev::Stream;
use sha3::digest::DynDigest;
use sha3::{Sha3_224, Sha3_256, Sha3_384, Sha3_512};
use std::fs::OpenOptions;
use std::io::Read;
use std::pin::Pin;
use std::task::{Context, Poll};
use url::Url;

use crate::error::Error;
use crate::location::TransferHash;
use crate::{TransferContext, TransferData, TransferStream, TransferUrl};

pub fn with_hash_stream(
    stream: TransferStream<TransferData, Error>,
    src_url: &TransferUrl,
    dst_url: &TransferUrl,
    ctx: &TransferContext,
) -> Result<Box<dyn Stream<Item = Result<TransferData, Error>> + Unpin>, Error> {
    Ok(match src_url.hash {
        Some(ref h) => {
            if ctx.state.offset() == 0 {
                Box::new(HashStream::try_new(stream, &h.alg, h.val.clone())?)
            } else {
                match dst_url.url.scheme() {
                    "file" => {
                        log::info!("[HashStream] File transfer from non-zero offset. Initializing hasher from disk..");
                        Box::new(HashStream::try_started(stream, h, &dst_url.url)?)
                    }
                    schema => {
                        log::warn!("HashStream is unable to transfer from non-zero offset when using schema: '{schema}'. Resetting offset to 0.'");
                        ctx.state.set_offset(0);
                        Box::new(HashStream::try_new(stream, &h.alg, h.val.clone())?)
                    }
                }
            }
        }
        None => Box::new(stream),
    })
}

struct HashStream<T, E, S>
where
    S: Stream<Item = Result<T, E>>,
{
    inner: S,
    hasher: Box<dyn DynDigest>,
    hash: Vec<u8>,
    result: Option<Vec<u8>>,
}

impl<T, S> HashStream<T, Error, S>
where
    S: Stream<Item = Result<T, Error>> + Unpin,
{
    pub fn try_new(stream: S, alg: &str, hash: Vec<u8>) -> Result<Self, Error> {
        let hasher: Box<dyn DynDigest> = match alg {
            "sha3" => match hash.len() * 8 {
                224 => Box::<Sha3_224>::default(),
                256 => Box::<Sha3_256>::default(),
                384 => Box::<Sha3_384>::default(),
                512 => Box::<Sha3_512>::default(),
                len => {
                    return Err(Error::UnsupportedDigestError(format!(
                        "Unsupported digest {} of length {}: {}",
                        alg,
                        len,
                        hex::encode(&hash),
                    )))
                }
            },
            _ => {
                return Err(Error::UnsupportedDigestError(format!(
                    "Unsupported digest: {}",
                    alg
                )))
            }
        };

        Ok(HashStream {
            inner: stream,
            hasher,
            hash,
            result: None,
        })
    }

    pub fn try_started(stream: S, h: &TransferHash, target: &Url) -> Result<Self, Error> {
        Self::try_new(stream, &h.alg, h.val.clone())?.init_from_file(target)
    }

    fn init_from_file(mut self, target: &Url) -> Result<Self, Error> {
        let mut file_src = OpenOptions::new().read(true).open(target.path())?;
        let mut chunk = vec![0; 4096];

        while let Ok(count) = file_src.read(&mut chunk[..]) {
            self.hasher.input(&chunk[..count]);
            if count != 4096 {
                break;
            }
        }

        Ok(self)
    }
}

impl<S> Stream for HashStream<TransferData, Error, S>
where
    S: Stream<Item = Result<TransferData, Error>> + Unpin,
{
    type Item = Result<TransferData, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let result = Stream::poll_next(Pin::new(&mut self.inner), cx);

        if let Poll::Ready(ref opt) = result {
            match opt {
                Some(item) => {
                    if let Ok(data) = item {
                        self.hasher.input(data.as_ref());
                    }
                }
                None => {
                    let result = match &self.result {
                        Some(r) => r,
                        None => {
                            self.result = Some(self.hasher.result_reset().to_vec());
                            self.result.as_ref().unwrap()
                        }
                    };

                    if &self.hash == result {
                        log::info!("Hash verified successfully: {:?}", hex::encode(result));
                    } else {
                        return Poll::Ready(Some(Err(Error::InvalidHashError {
                            expected: hex::encode(&self.hash),
                            hash: hex::encode(result),
                        })));
                    }
                }
            }
        }

        result
    }
}
