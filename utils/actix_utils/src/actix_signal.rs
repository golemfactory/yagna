use std::sync::{Arc, Mutex};

use actix::prelude::*;
use anyhow::{anyhow, Result};

/// Actor that provides signal subscriptions
#[derive(Clone)]
pub struct SignalSlot<MessageType>
where
    MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
    MessageType::Result: std::marker::Send + std::marker::Sync,
{
    subscribers: Arc<Mutex<Vec<Recipient<MessageType>>>>,
}

/// Subscribe to process signals.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Subscribe<MessageType>(pub Recipient<MessageType>)
where
    MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
    MessageType::Result: std::marker::Send + std::marker::Sync;

/// Send signal from asynchronous code.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct Signal<MessageType>(pub MessageType)
where
    MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
    MessageType::Result: std::marker::Send + std::marker::Sync;

#[allow(dead_code)]
impl<MessageType> SignalSlot<MessageType>
where
    MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
    MessageType::Result: std::marker::Send + std::marker::Sync,
{
    pub fn new() -> SignalSlot<MessageType> {
        SignalSlot::<MessageType> {
            subscribers: Default::default(),
        }
    }

    /// Send signal to all subscribers
    pub fn send_signal(&self, message: MessageType) -> Result<()> {
        let subscribers = self.subscribers.lock().unwrap();
        let errors = subscribers
            .iter()
            .map(|subscriber| subscriber.do_send(message.clone()))
            .filter_map(|result| {
                match result {
                    Err(error) => {
                        //TODO: It would be useful to have better error message, that suggest which signal failed.
                        log::error!(
                            "Sending signal to subscriber failed in SignalSlot::send_signal. {}",
                            error
                        );
                        Some(error)
                    }
                    Ok(_) => None,
                }
            })
            .collect::<Vec<SendError<MessageType>>>();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(anyhow!("Errors while sending signal: {:?}", errors))
        }
    }

    pub fn subscribe(&mut self, subscriber: Recipient<MessageType>) {
        let mut subscribers = self.subscribers.lock().unwrap();
        subscribers.push(subscriber);
    }

    pub fn on_subscribe(&mut self, msg: Subscribe<MessageType>) {
        self.subscribe(msg.0);
    }
}

#[macro_export]
macro_rules! actix_signal_handler {
    ($ActorType:ty, $MessageType:ty, $SignalFieldName:tt) => {
        impl Handler<$crate::actix_signal::Subscribe<$MessageType>> for $ActorType {
            type Result = ();

            fn handle(
                &mut self,
                msg: $crate::actix_signal::Subscribe<$MessageType>,
                _ctx: &mut <Self as Actor>::Context,
            ) -> Self::Result {
                self.$SignalFieldName.subscribe(msg.0);
            }
        }

        impl Handler<$crate::actix_signal::Signal<$MessageType>> for $ActorType {
            type Result = Result<()>;

            fn handle(
                &mut self,
                msg: $crate::actix_signal::Signal<$MessageType>,
                _ctx: &mut <Self as Actor>::Context,
            ) -> Self::Result {
                self.$SignalFieldName.send_signal(msg.0)
            }
        }
    };
}
