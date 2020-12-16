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
    pub fn count(&mut self, count: i32) -> &mut Self {
        self.count = count;
        self
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

        if self.count > 0 {
            self.count -= 1;
            Some(duration)
        } else if self.count < 0 {
            Some(duration)
        } else {
            None
        }
    }
}

fn can_retry(err: &Error) -> bool {
    match err {
        Error::HttpError(e) => match e {
            HttpError::Timeout(_) | HttpError::Connect(_) | HttpError::Server(_) => true,
            HttpError::Io(kind) => match kind {
                ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::NotConnected
                | ErrorKind::AddrNotAvailable
                | ErrorKind::BrokenPipe
                | ErrorKind::TimedOut
                | ErrorKind::Interrupted => true,
                _ => false,
            },
            _ => false,
        },
        Error::Gsb(e) => match e {
            BusError::Timeout(_)
            | BusError::Closed(_)
            | BusError::ConnectionFail(_, _)
            | BusError::ConnectionTimeout(_)
            | BusError::NoEndpoint(_) => true,
            _ => false,
        },
        _ => false,
    }
}
