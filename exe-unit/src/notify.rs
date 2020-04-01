use futures::task::{Context, Poll, Waker};
use futures::Future;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};

pub struct Notify<T: Clone + Eq> {
    uid: usize,
    prev: Option<T>,
    state: Arc<Mutex<NotifyState<T>>>,
}

impl<T: Clone + Eq> Notify<T> {
    pub fn notify(&mut self, t: T) {
        let mut state = self.state.lock().unwrap();
        let _ = match &state.inner {
            Some(s) => match s == &t {
                true => return,
                false => state.inner.replace(t),
            },
            None => state.inner.replace(t),
        };
        state.waker_map.values_mut().for_each(|w| w.wake_by_ref());
    }
}

impl<T: Clone + Eq> Default for Notify<T> {
    fn default() -> Self {
        Notify {
            uid: 1,
            prev: None,
            state: Arc::new(Mutex::new(NotifyState::default())),
        }
    }
}

impl<T: Clone + Eq> Clone for Notify<T> {
    fn clone(&self) -> Self {
        let state = self.state.lock().unwrap();
        let uid = state.seq.fetch_add(1, SeqCst);

        Notify {
            uid,
            prev: None,
            state: self.state.clone(),
        }
    }
}

impl<T: Clone + Eq + Unpin> Future for Notify<T> {
    type Output = Self;

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = {
            let state = self.state.lock().unwrap();
            if self.prev != state.inner {
                state.inner.clone()
            } else {
                None
            }
        };

        if let Some(t) = inner {
            self.as_mut().prev.replace(t.clone());
            return Poll::Ready(self.to_owned());
        }

        let mut state = self.state.lock().unwrap();
        state.waker_map.insert(self.uid, cx.waker().clone());

        Poll::Pending
    }
}

impl<T: Clone + Eq> Drop for Notify<T> {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.waker_map.remove(&self.uid);
    }
}

struct NotifyState<T: Eq> {
    inner: Option<T>,
    seq: AtomicUsize,
    waker_map: HashMap<usize, Waker>,
}

impl<T: Eq> Default for NotifyState<T> {
    fn default() -> Self {
        NotifyState {
            inner: None,
            seq: AtomicUsize::new(1),
            waker_map: HashMap::new(),
        }
    }
}
