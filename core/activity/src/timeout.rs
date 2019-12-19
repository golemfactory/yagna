use futures::future::{Either, Future, Map};
use futures::task::{Context, Poll};
use futures::FutureExt;
use std::pin::Pin;
use std::time::{Duration, Instant};

pub type Timeout = Option<u32>;
type FutureOutput<F> = <F as Future>::Output;
type MapType<F> = Map<F, fn(FutureOutput<F>) -> Result<FutureOutput<F>, TimeoutError>>;

pub trait IntoDuration {
    fn into_duration(self) -> Duration;
}

impl IntoDuration for u64 {
    #[inline(always)]
    fn into_duration(self) -> Duration {
        Duration::from_millis(self)
    }
}

impl IntoDuration for Duration {
    #[inline(always)]
    fn into_duration(self) -> Duration {
        self
    }
}

macro_rules! impl_into_duration {
    ($ty:ty) => {
        impl IntoDuration for $ty {
            fn into_duration(self) -> Duration {
                if self > 0 as $ty {
                    (self as u64).into_duration()
                } else {
                    Duration::default()
                }
            }
        }
    };
}

impl_into_duration!(i8);
impl_into_duration!(u8);
impl_into_duration!(i16);
impl_into_duration!(u16);
impl_into_duration!(i32);
impl_into_duration!(u32);
impl_into_duration!(i64);
impl_into_duration!(i128);
impl_into_duration!(u128);

impl_into_duration!(f32);
impl_into_duration!(f64);

#[derive(Debug)]
pub struct TimeoutInterval {
    timeout: Duration,
    deadline: Instant,
}

impl TimeoutInterval {
    pub fn new<T>(timeout: T) -> Self
    where
        T: IntoDuration,
    {
        let timeout = timeout.into_duration();
        Self {
            timeout,
            deadline: Instant::now(),
        }
    }

    #[inline]
    pub fn check(&mut self) -> bool {
        let now = Instant::now();
        let result = now >= self.deadline;
        self.deadline = now + self.timeout;
        result
    }
}

#[derive(Debug)]
pub struct TimeoutError {}

#[derive(Debug)]
pub struct TimeoutFuture<F: Future + Unpin> {
    future: F,
    deadline: Instant,
}

impl<F: Future + Unpin> TimeoutFuture<F> {
    pub fn new(future: F, timeout: Duration) -> Self {
        Self {
            future,
            deadline: Instant::now() + timeout,
        }
    }
}

impl<F: Future + Unpin> Future for TimeoutFuture<F> {
    type Output = Result<<F as Future>::Output, TimeoutError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(output) = self.future.poll_unpin(ctx) {
            return Poll::Ready(Ok(output));
        };
        if Instant::now() >= self.deadline {
            return Poll::Ready(Err(TimeoutError {}));
        }

        ctx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<F: Future + Unpin> Unpin for TimeoutFuture<F> {}

pub trait IntoTimeoutFuture<T>: Future
where
    Self: Future + Unpin + Sized,
    T: IntoDuration,
{
    fn timeout(self, timeout: Option<T>) -> Either<TimeoutFuture<Self>, MapType<Self>>;
}

impl<F, T> IntoTimeoutFuture<T> for F
where
    Self: Future + Unpin + Sized,
    T: IntoDuration,
{
    fn timeout(self, timeout: Option<T>) -> Either<TimeoutFuture<Self>, MapType<Self>> {
        match timeout {
            Some(t) => Either::Left(TimeoutFuture::new(self, t.into_duration())),
            None => Either::Right(self.map(|v| Result::Ok(v))),
        }
    }
}
