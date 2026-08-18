use futures::channel::mpsc;
use futures::StreamExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use ya_client_model::activity::{CaptureFormat, CaptureMode, CapturePart, CommandOutput};

use crate::message::RuntimeEvent;

const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

pub(crate) async fn forward_output<F, R>(read: R, tx: &mpsc::Sender<RuntimeEvent>, f: F)
where
    F: Fn(Vec<u8>) -> RuntimeEvent + 'static,
    R: tokio::io::AsyncRead + 'static,
{
    let tx = tx.clone();
    let stream = FramedRead::new(read, BytesCodec::new())
        .filter_map(|result| async { result.ok() })
        .ready_chunks(16)
        .map(|v| v.into_iter().flat_map(|b| b.to_vec()).collect())
        .map(f)
        .map(Ok);

    if let Err(e) = stream.forward(tx).await {
        log::error!("Error forwarding output: {:?}", e);
    }
}

pub(crate) struct CapturedOutput {
    pub stream: bool,
    stream_remaining: Option<usize>,
    pub format: CaptureFormat,
    head: CaptureBuffer,
    tail: CaptureBuffer,
}

impl CapturedOutput {
    pub fn all() -> Self {
        CapturedOutput {
            stream: true,
            stream_remaining: None,
            format: CaptureFormat::default(),
            head: CaptureBuffer::all(),
            tail: CaptureBuffer::discard(),
        }
    }

    pub fn discard() -> Self {
        CapturedOutput {
            stream: false,
            stream_remaining: None,
            format: CaptureFormat::default(),
            head: CaptureBuffer::discard(),
            tail: CaptureBuffer::discard(),
        }
    }

    pub fn output(&self) -> Option<CommandOutput> {
        let head = self.head.as_slice().unwrap_or(&[]);
        let tail = self.tail.as_slice().unwrap_or(&[]);
        let mut output = head.to_vec();
        output.extend_from_slice(tail);

        if output.is_empty() {
            None
        } else {
            match self.format {
                CaptureFormat::Bin => Some(CommandOutput::Bin(output)),
                CaptureFormat::Str => vec_to_string(output).map(CommandOutput::Str),
            }
        }
    }

    pub fn output_string(&self) -> Option<String> {
        self.output().and_then(|o| match o {
            CommandOutput::Bin(b) => vec_to_string(b),
            CommandOutput::Str(s) => Some(s),
        })
    }

    pub fn write<B: AsRef<[u8]> + ?Sized>(&mut self, bytes: &B) -> Option<CommandOutput> {
        let bytes = bytes.as_ref();
        let bytes_head = self.head.write(bytes);
        let bytes_tail = self.tail.write(bytes);
        let captured = bytes_head.or(bytes_tail);
        let output = if self.stream {
            let end_idx = match &mut self.stream_remaining {
                Some(remaining) => {
                    let end_idx = bytes.len().min(*remaining);
                    *remaining -= end_idx;
                    end_idx
                }
                None => bytes.len(),
            };
            (end_idx > 0).then_some(&bytes[..end_idx])
        } else {
            captured
        };

        match self.format {
            CaptureFormat::Str => {
                output.map(|b| CommandOutput::Str(String::from_utf8_lossy(b).to_string()))
            }
            CaptureFormat::Bin => output.map(|b| CommandOutput::Bin(b.to_vec())),
        }
    }
}

impl From<Option<CaptureMode>> for CapturedOutput {
    fn from(maybe_mode: Option<CaptureMode>) -> Self {
        let mode = match maybe_mode {
            Some(mode) => mode,
            None => return CapturedOutput::discard(),
        };
        match mode {
            CaptureMode::AtEnd { part, format } => {
                let (head, tail) = match part {
                    Some(CapturePart::Head(limit)) => {
                        (CaptureBuffer::capped(limit), CaptureBuffer::discard())
                    }
                    Some(CapturePart::Tail(limit)) => {
                        (CaptureBuffer::discard(), CaptureBuffer::ring(limit))
                    }
                    Some(CapturePart::HeadTail(limit)) => {
                        let limit = capture_limit(limit);
                        let head_limit = limit.div_ceil(2);
                        let tail_limit = limit - head_limit;
                        (
                            CaptureBuffer::capped(head_limit),
                            CaptureBuffer::ring(tail_limit),
                        )
                    }
                    None => (CaptureBuffer::all(), CaptureBuffer::discard()),
                };

                CapturedOutput {
                    stream: false,
                    stream_remaining: None,
                    format: format.unwrap_or_default(),
                    head,
                    tail,
                }
            }
            CaptureMode::Stream { limit, format } => CapturedOutput {
                stream: true,
                stream_remaining: limit,
                format: format.unwrap_or_default(),
                head: match limit {
                    Some(limit) => CaptureBuffer::capped(limit),
                    None => CaptureBuffer::all(),
                },
                tail: CaptureBuffer::discard(),
            },
        }
    }
}

pub(crate) enum CaptureBuffer {
    Capped(Vec<u8>, usize),
    Ring(Vec<u8>, usize),
    Discard,
}

fn capture_limit(limit: usize) -> usize {
    limit.min(MAX_CAPTURE_BYTES)
}

impl CaptureBuffer {
    pub fn all() -> Self {
        CaptureBuffer::capped(MAX_CAPTURE_BYTES)
    }

    pub fn capped(limit: usize) -> Self {
        let limit = capture_limit(limit);
        if limit == 0 {
            return CaptureBuffer::Discard;
        }
        CaptureBuffer::Capped(Vec::new(), limit)
    }

    pub fn ring(limit: usize) -> Self {
        let limit = capture_limit(limit);
        if limit == 0 {
            return CaptureBuffer::Discard;
        }
        CaptureBuffer::Ring(Vec::new(), limit)
    }

    pub fn discard() -> Self {
        CaptureBuffer::Discard
    }
}

impl CaptureBuffer {
    pub fn as_slice(&self) -> Option<&[u8]> {
        match self {
            CaptureBuffer::Capped(vec, _) => Some(vec.as_slice()),
            CaptureBuffer::Ring(vec, _) => Some(vec.as_slice()),
            CaptureBuffer::Discard => None,
        }
    }

    pub fn write<'b, B: AsRef<[u8]> + ?Sized>(&mut self, bytes: &'b B) -> Option<&'b [u8]> {
        let bytes = bytes.as_ref();
        let sz = bytes.len();
        if sz == 0 {
            return None;
        }

        match self {
            CaptureBuffer::Capped(vec, limit) => {
                let end_idx = sz.min(*limit - vec.len());
                let slice = &bytes[..end_idx];

                if slice.is_empty() {
                    None
                } else {
                    vec.extend_from_slice(slice);
                    Some(slice)
                }
            }
            CaptureBuffer::Ring(vec, limit) => {
                let len = vec.len();
                let slice = if sz > *limit {
                    &bytes[sz - *limit..]
                } else {
                    bytes
                };

                let mut end_idx = len + slice.len();
                let shift = end_idx.saturating_sub(*limit);
                let start_idx = len - shift;
                end_idx -= shift;

                vec.rotate_left(shift);
                if len < end_idx {
                    vec.extend(std::iter::repeat_n(0, end_idx - len));
                }
                vec.as_mut_slice()[start_idx..end_idx].copy_from_slice(slice);
                Some(slice)
            }
            CaptureBuffer::Discard => None,
        }
    }
}

pub(crate) fn vec_to_string(vec: Vec<u8>) -> Option<String> {
    if vec.is_empty() {
        return None;
    }
    let string = match String::from_utf8(vec) {
        Ok(utf8) => utf8,
        Err(error) => error
            .as_bytes()
            .iter()
            .map(|&c| c as char)
            .collect::<String>(),
    };
    Some(string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_buffer() {
        let mut buf = CaptureBuffer::capped(5);
        assert_eq!(buf.as_slice(), Some(&[][..]));
        buf.write(&[]);
        assert_eq!(buf.as_slice(), Some(&[][..]));

        buf.write(&[0]);
        assert_eq!(buf.as_slice(), Some(&[0][..]));
        buf.write(&[1]);
        assert_eq!(buf.as_slice(), Some(&[0, 1][..]));
        buf.write(&[2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(buf.as_slice(), Some(&[0, 1, 2, 3, 4][..]));
        buf.write(&[11, 12, 13, 14, 15, 16]);
        assert_eq!(buf.as_slice(), Some(&[0, 1, 2, 3, 4][..]));
    }

    #[test]
    fn ring_buffer() {
        let mut buf = CaptureBuffer::ring(5);
        assert_eq!(buf.as_slice(), Some(&[][..]));
        buf.write(&[]);
        assert_eq!(buf.as_slice(), Some(&[][..]));

        buf.write(&[0]);
        assert_eq!(buf.as_slice(), Some(&[0][..]));
        buf.write(&[1, 2, 3, 4]);
        assert_eq!(buf.as_slice(), Some(&[0, 1, 2, 3, 4][..]));
        buf.write(&[5]);
        assert_eq!(buf.as_slice(), Some(&[1, 2, 3, 4, 5][..]));
        buf.write(&[0, 0, 0]);
        assert_eq!(buf.as_slice(), Some(&[4, 5, 0, 0, 0][..]));
        buf.write(&[6, 7, 8, 9, 10][..]);
        assert_eq!(buf.as_slice(), Some(&[6, 7, 8, 9, 10][..]));
        buf.write(&[6, 7, 8, 9, 10, 11, 12, 13, 14][..]);
        assert_eq!(buf.as_slice(), Some(&[10, 11, 12, 13, 14][..]));
    }

    #[test]
    fn capture_buffers_are_lazily_allocated_and_clamped() {
        match CaptureBuffer::capped(usize::MAX) {
            CaptureBuffer::Capped(vec, limit) => {
                assert_eq!(limit, MAX_CAPTURE_BYTES);
                assert_eq!(vec.capacity(), 0);
            }
            _ => panic!("expected capped buffer"),
        }

        match CaptureBuffer::ring(usize::MAX) {
            CaptureBuffer::Ring(vec, limit) => {
                assert_eq!(limit, MAX_CAPTURE_BYTES);
                assert_eq!(vec.capacity(), 0);
            }
            _ => panic!("expected ring buffer"),
        }
    }

    #[test]
    fn head_tail_capture_limit_is_total_limit() {
        let captured = CapturedOutput::from(Some(CaptureMode::AtEnd {
            part: Some(CapturePart::HeadTail(usize::MAX)),
            format: None,
        }));

        match captured.head {
            CaptureBuffer::Capped(_, limit) => assert_eq!(limit, MAX_CAPTURE_BYTES.div_ceil(2)),
            _ => panic!("expected capped head"),
        }
        match captured.tail {
            CaptureBuffer::Ring(_, limit) => assert_eq!(limit, MAX_CAPTURE_BYTES / 2),
            _ => panic!("expected ring tail"),
        }
    }

    fn assert_default_capture_limit(captured: &CapturedOutput, stream: bool) {
        assert_eq!(captured.stream, stream);
        assert_eq!(captured.stream_remaining, None);
        match &captured.head {
            CaptureBuffer::Capped(vec, limit) => {
                assert_eq!(*limit, MAX_CAPTURE_BYTES);
                assert_eq!(vec.capacity(), 0);
            }
            _ => panic!("expected capped head"),
        }
        assert!(matches!(&captured.tail, CaptureBuffer::Discard));
    }

    #[test]
    fn no_limit_capture_modes_use_global_cap() {
        let at_end = CapturedOutput::from(Some(CaptureMode::AtEnd {
            part: None,
            format: None,
        }));
        assert_default_capture_limit(&at_end, false);

        let stream = CapturedOutput::from(Some(CaptureMode::Stream {
            limit: None,
            format: None,
        }));
        assert_default_capture_limit(&stream, true);

        let command_state = crate::state::CommandState::all();
        assert_default_capture_limit(&command_state.stdout, true);
        assert_default_capture_limit(&command_state.stderr, true);
    }

    #[test]
    fn unlimited_stream_continues_after_retention_cap() {
        let mut captured = CapturedOutput::from(Some(CaptureMode::Stream {
            limit: None,
            format: Some(CaptureFormat::Bin),
        }));
        captured.head.write(&vec![0; MAX_CAPTURE_BYTES]);

        assert_eq!(
            captured.write(&[1, 2, 3]),
            Some(CommandOutput::Bin(vec![1, 2, 3]))
        );
        assert_eq!(captured.head.as_slice().unwrap().len(), MAX_CAPTURE_BYTES);
    }

    #[test]
    fn explicit_stream_limit_is_preserved() {
        let mut captured = CapturedOutput::from(Some(CaptureMode::Stream {
            limit: Some(2),
            format: Some(CaptureFormat::Bin),
        }));

        assert_eq!(
            captured.write(&[1, 2, 3]),
            Some(CommandOutput::Bin(vec![1, 2]))
        );
        assert_eq!(captured.write(&[4]), None);
        assert_eq!(captured.output(), Some(CommandOutput::Bin(vec![1, 2])));
    }
}
