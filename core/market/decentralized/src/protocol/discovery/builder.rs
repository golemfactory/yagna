use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::protocol::callback::{CallbackFuture, OutputFuture};
use crate::protocol::callback::{CallbackHandler, CallbackMessage, HandlerSlot};

use super::{Discovery, DiscoveryImpl};
use crate::protocol::discovery::ReceiveHandlers;

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
        let data = self.get_data::<T>();
        self.handlers.insert(
            TypeId::of::<M>(),
            Box::new(HandlerSlot::new(move |caller, msg| {
                f.handle(data.clone(), caller, msg)
            })),
        );
        self
    }

    #[allow(dead_code)]
    pub fn add_handler<M: CallbackMessage>(mut self, f: impl CallbackHandler<M>) -> Self {
        self.handlers
            .insert(TypeId::of::<M>(), Box::new(HandlerSlot::new(f)));
        self
    }

    fn get<M: CallbackMessage>(&mut self) -> HandlerSlot<M> {
        let boxed = self.handlers.remove(&TypeId::of::<M>()).unwrap();
        *(boxed as Box<dyn Any + 'static>).downcast().unwrap()
    }

    fn get_data<T: Clone + Send + Sync + 'static>(&mut self) -> T {
        let boxed = self
            .data
            .get(&TypeId::of::<T>())
            .expect("[DiscoveryBuilder] Can't find data of required type.");

        let data: &T = (&**boxed as &(dyn Any + 'static)).downcast_ref().unwrap();
        data.clone()
    }

    pub fn build(mut self) -> Discovery {
        let receive = Mutex::new(ReceiveHandlers {
            offers: self.get(),
            offer_ids: self.get(),
        });
        Discovery {
            inner: Arc::new(DiscoveryImpl {
                identity: self.get_data(),
                receive,
                offer_unsubscribed: self.get(),
                get_offers_request: self.get(),
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

    use crate::testing::mock_identity::{generate_identity, MockIdentity};
    use crate::testing::mock_offer::sample_get_offer_received;

    use super::super::*;
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
            .data(MockIdentity::new("test") as Arc<dyn IdentityApi>)
            .add_handler(|_, _: OffersReceived| async { Ok(vec![]) })
            .build();
    }

    #[test]
    #[should_panic(expected = "[DiscoveryBuilder] Can't find data of required type.")]
    fn setting_db_handler_wo_db_should_fail() {
        DiscoveryBuilder::default()
            .data(MockIdentity::new("test") as Arc<dyn IdentityApi>)
            .add_data_handler(|_: u8, _, _: OffersReceived| async { Ok(vec![]) })
            .build();
    }

    #[test]
    #[should_panic]
    fn build_from_with_missing_handler_should_fail() {
        DiscoveryBuilder::default()
            .data(MockIdentity::new("test") as Arc<dyn IdentityApi>)
            .add_handler(|_, _: OffersReceived| async { Ok(vec![]) })
            .add_handler(|_, _: OfferUnsubscribed| async { Ok(vec![]) })
            .build();
    }

    #[test]
    fn build_from_with_four_handlers_should_pass() {
        DiscoveryBuilder::default()
            .data(MockIdentity::new("test") as Arc<dyn IdentityApi>)
            .add_handler(|_, _: OffersReceived| async { Ok(vec![]) })
            .add_handler(|_, _: OfferUnsubscribed| async { Ok(vec![]) })
            .add_handler(|_, _: OfferIdsReceived| async { Ok(vec![]) })
            .add_handler(|_, _: GetOffers| async { Ok(vec![]) })
            .build();
    }

    #[test]
    fn build_from_with_mixed_handlers_should_pass() {
        DiscoveryBuilder::default()
            .data(MockIdentity::new("test") as Arc<dyn IdentityApi>)
            .data("mock data")
            .add_handler(|_, _: OffersReceived| async { Ok(vec![]) })
            .add_data_handler(|_: &str, _, _: OfferUnsubscribed| async { Ok(vec![]) })
            .add_handler(|_, _: OfferIdsReceived| async { Ok(vec![]) })
            .add_data_handler(|_: &str, _, _: GetOffers| async { Ok(vec![]) })
            .build();
    }

    #[actix_rt::test]
    async fn build_from_with_overwritten_handlers_should_pass() {
        // given
        let _ = env_logger::builder().try_init();
        let counter = Arc::new(AtomicUsize::new(0));
        let cnt = counter.clone();

        let discovery = DiscoveryBuilder::default()
            .data(MockIdentity::new("test") as Arc<dyn IdentityApi>)
            .data(7 as usize)
            .data("mock data")
            .add_handler(|_, _: OffersReceived| async { Ok(vec![]) })
            .add_handler(|_, _: GetOffers| async { panic!("should not be invoked") })
            .add_data_handler(|_: &str, _, _: OfferUnsubscribed| async { Ok(vec![]) })
            .add_data_handler(move |data: usize, _, _: GetOffers| {
                let cnt = cnt.clone();
                async move {
                    cnt.fetch_add(data, SeqCst);
                    Ok(vec![])
                }
            })
            .add_handler(|_, _: OfferIdsReceived| async { Ok(vec![]) })
            .build();

        assert_eq!(0, counter.load(SeqCst));

        // when
        let node_id = generate_identity("caller").identity.to_string();
        discovery
            .on_get_offers(node_id, sample_get_offer_received())
            .await
            .unwrap();

        // then
        assert_eq!(7, counter.load(SeqCst));
    }
}
