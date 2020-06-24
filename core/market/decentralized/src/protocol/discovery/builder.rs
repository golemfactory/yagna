use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use crate::protocol::callbacks::{CallbackFuture, OutputFuture};
use crate::protocol::discovery::DiscoveryImpl;
use crate::protocol::{CallbackHandler, CallbackMessage, Discovery, HandlerSlot};

#[derive(Default)]
pub struct DiscoveryBuilder {
    data: HashMap<TypeId, Box<dyn Any>>,
    handlers: HashMap<TypeId, Box<dyn Any>>,
}

impl DiscoveryBuilder {
    pub fn data<T: Clone + Send + Sync + 'static>(mut self, data: T) -> Self {
        self.data.insert(TypeId::of::<T>(), Box::new(data));
        self
    }

    pub fn add_data_handler<M: CallbackMessage, T: Clone + Send + Sync + 'static>(
        mut self,
        mut f: impl DataCallbackHandler<M, T>,
    ) -> Self {
        let boxed = self
            .data
            .get(&TypeId::of::<T>())
            .expect("data handler needs data");

        // let data = *(boxed as &Box<dyn Any + 'static>).downcast::<&T>().unwrap();
        let data: &T = (&**boxed as &(dyn Any + 'static)).downcast_ref().unwrap();
        let data = data.clone();
        self.handlers.insert(
            TypeId::of::<M>(),
            Box::new(HandlerSlot::new(move |caller, msg| {
                f.handle(data.clone(), caller, msg)
            })),
        );
        self
    }

    pub fn add_handler<M: CallbackMessage>(mut self, f: impl CallbackHandler<M>) -> Self {
        self.handlers
            .insert(TypeId::of::<M>(), Box::new(HandlerSlot::new(f)));
        self
    }

    fn get<M: CallbackMessage>(&mut self) -> HandlerSlot<M> {
        let boxed = self.handlers.remove(&TypeId::of::<M>()).unwrap();
        *(boxed as Box<dyn Any + 'static>).downcast().unwrap()
    }

    pub fn build(mut self) -> Discovery {
        Discovery {
            inner: Arc::new(DiscoveryImpl {
                offer_received: self.get(),
                offer_unsubscribed: self.get(),
                retrieve_offers: self.get(),
            }),
        }
    }
}

pub trait DataCallbackHandler<M: CallbackMessage, T>: Send + Sync + 'static {
    fn handle(&mut self, data: T, caller: String, msg: M) -> CallbackFuture<M>;
}

impl<
        T,
        M: CallbackMessage,
        O: OutputFuture<M>,
        F: FnMut(T, String, M) -> O + Send + Sync + 'static,
    > DataCallbackHandler<M, T> for F
{
    fn handle(&mut self, data: T, caller: String, msg: M) -> CallbackFuture<M> {
        Box::pin(self(data, caller, msg))
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    use crate::protocol::{OfferReceived, OfferUnsubscribed, Propagate, Reason, RetrieveOffers};
    use crate::testing::mock_offer::sample_offer_received;

    use super::*;

    #[test]
    #[should_panic]
    fn build_from_default_should_fail() {
        DiscoveryBuilder::default().build();
    }

    #[test]
    #[should_panic]
    fn build_with_single_handler_should_fail() {
        DiscoveryBuilder::default()
            .add_handler(|_, _: OfferReceived| async { Ok(Propagate::Yes) })
            .build();
    }

    #[test]
    #[should_panic(expected = "data handler needs data")]
    fn setting_db_handler_wo_db_should_fail() {
        DiscoveryBuilder::default()
            .add_data_handler(|_: u8, _, _: OfferReceived| async { Ok(Propagate::Yes) })
            .build();
    }

    #[test]
    #[should_panic]
    fn build_from_with_missing_handler_should_fail() {
        DiscoveryBuilder::default()
            .add_handler(|_, _: OfferReceived| async { Ok(Propagate::Yes) })
            .add_handler(|_, _: OfferUnsubscribed| async { Ok(Propagate::Yes) })
            .build();
    }

    #[test]
    fn build_from_with_three_handlers_should_pass() {
        DiscoveryBuilder::default()
            .add_handler(|_, _: OfferReceived| async { Ok(Propagate::Yes) })
            .add_handler(|_, _: OfferUnsubscribed| async { Ok(Propagate::Yes) })
            .add_handler(|_, _: RetrieveOffers| async { Ok(vec![]) })
            .build();
    }

    #[test]
    fn build_from_with_mixed_handlers_should_pass() {
        DiscoveryBuilder::default()
            .data("mock data")
            .add_handler(|_, _: OfferReceived| async { Ok(Propagate::Yes) })
            .add_data_handler(|_: &str, _, _: OfferUnsubscribed| async { Ok(Propagate::Yes) })
            .add_handler(|_, _: RetrieveOffers| async { Ok(vec![]) })
            .build();
    }

    #[actix_rt::test]
    async fn build_from_with_overwritten_handlers_should_pass() {
        // given
        let _ = env_logger::builder().try_init();
        let counter = Arc::new(AtomicUsize::new(0));
        let cnt = counter.clone();

        let discovery = DiscoveryBuilder::default()
            .data(7 as usize)
            .data("mock data")
            .add_handler(|_, _: OfferReceived| async { panic!("should not be invoked") })
            .add_data_handler(|_: &str, _, _: OfferUnsubscribed| async { Ok(Propagate::Yes) })
            .add_data_handler(move |data: usize, _, _: OfferReceived| {
                let cnt = cnt.clone();
                async move {
                    cnt.fetch_add(data, SeqCst);
                    Ok(Propagate::No(Reason::AlreadyExists))
                }
            })
            .add_handler(|_, _: RetrieveOffers| async { Ok(vec![]) })
            .build();

        assert_eq!(0, counter.load(SeqCst));

        // when
        discovery
            .on_offer_received("caller".into(), sample_offer_received())
            .await
            .unwrap();

        // then
        assert_eq!(7, counter.load(SeqCst));
    }
}
