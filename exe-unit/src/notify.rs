use futures::task::{Context, Poll, Waker};
use futures::Future;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};

pub struct Notify<T: Clone + Eq> {
    uid: usize,
    when: Option<Box<dyn Fn(T) -> bool>>,
    last: Option<T>,
    state: Arc<Mutex<NotifyState<T>>>,
}

impl<T: Clone + Eq> Notify<T> {
    pub fn notify(&mut self, t: T) {
        let mut state = self.state.lock().unwrap();
        if state.inner.as_ref() == Some(&t) {
            return;
        } else {
            state.inner.replace(t);
        }
        state.waker_map.values_mut().for_each(|w| w.wake_by_ref());
    }

    pub fn when<F: Fn(T) -> bool + 'static>(mut self, f: F) -> Self {
        self.when = Some(Box::new(f));
        self
    }
}

impl<T: Clone + Eq> Default for Notify<T> {
    fn default() -> Self {
        Notify {
            uid: usize::MAX,
            when: None,
            last: None,
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
            when: None,
            last: None,
            state: self.state.clone(),
        }
    }
}

impl<T: Clone + Eq + Unpin> Future for Notify<T> {
    type Output = Self;

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let new_state = {
            let state = self.state.lock().unwrap();
            match state.inner == self.last {
                true => None,
                _ => state.inner.clone(),
            }
        };

        if let Some(t) = new_state {
            self.as_mut().last.replace(t.clone());
            if self.when.as_ref().map(|f| f(t)).unwrap_or(true) {
                return Poll::Ready(self.to_owned());
            }
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
