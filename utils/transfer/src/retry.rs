use crate::error::{Error, HttpError};
use std::io::ErrorKind;
use std::time::Duration;
use ya_service_bus::error::Error as BusError;

#[derive(Clone, Debug)]
pub struct Retry {
    count: i32,
    backoff: f32,
    backoff_factor: f32,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            count: 2,
            backoff: 1.,
            backoff_factor: 2.,
        }
    }
}

impl Retry {
    pub fn new(count: i32) -> Self {
        Self {
            count,
            ..Retry::default()
        }
    }

    pub fn backoff(&mut self, initial: f32, factor: f32) -> &mut Self {
        self.backoff = initial / factor;
        self.backoff_factor = factor;
        self
    }

    pub fn delay(&mut self, err: &Error) -> Option<Duration> {
        if can_retry(err) {
            self.next()
        } else {
            None
        }
    }
}

impl Iterator for Retry {
    type Item = Duration;

    fn next(&mut self) -> Option<Self::Item> {
        self.backoff *= self.backoff_factor;
        let duration = Duration::from_secs_f32(self.backoff);

        match Ord::cmp(&self.count, &0) {
            std::cmp::Ordering::Less => Some(duration),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => {
                self.count -= 1;
                Some(duration)
            }
        }
    }
}

fn can_retry(err: &Error) -> bool {
    match err {
        Error::HttpError(e) => match e {
            HttpError::Timeout(_) | HttpError::Connect(_) | HttpError::Server(_) => true,
            HttpError::Io(kind) => matches!(
                kind,
                ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::NotConnected
                    | ErrorKind::AddrNotAvailable
                    | ErrorKind::BrokenPipe
                    | ErrorKind::TimedOut
                    | ErrorKind::Interrupted
            ),
            _ => false,
        },
        Error::Gsb(e) => matches!(
            e,
            BusError::Timeout(_)
                | BusError::Closed(_)
                | BusError::ConnectionFail(_, _)
                | BusError::ConnectionTimeout(_)
                | BusError::NoEndpoint(_)
        ),
        _ => false,
    }
}
