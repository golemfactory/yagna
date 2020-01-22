use actix::prelude::*;
use anyhow::Result;
use log::error;

/// Subscribe to process signals.
#[derive(Message)]
#[rtype(result = "()")]
#[allow(dead_code)]
pub struct Subscribe<MessageType>(pub Recipient<MessageType>)
where
    MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
    MessageType::Result: std::marker::Send + std::marker::Sync;

/// Actor that provides signal subscriptions
pub struct SignalSlot<MessageType>
where
    MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
    MessageType::Result: std::marker::Send + std::marker::Sync,
{
    subscribers: Vec<Recipient<MessageType>>,
}

#[allow(dead_code)]
impl<MessageType> SignalSlot<MessageType>
where
    MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
    MessageType::Result: std::marker::Send + std::marker::Sync,
{
    pub fn new() -> SignalSlot<MessageType> {
        SignalSlot::<MessageType> {
            subscribers: vec![],
        }
    }

    /// Send signal to all subscribers
    pub fn send_signal(&self, message: MessageType) -> Result<()> {
        for subscriber in &self.subscribers {
            if let Err(error) = subscriber.do_send(message.clone()) {
                //TODO: It would be useful to have better error message, that suggest which signal failed.
                error!(
                    "Sending signal to subscriber failed in SignalSlot::send_signal. {}",
                    error
                );
            }
        }
        Ok(())
    }

    pub fn subscribe(&mut self, subscriber: Recipient<MessageType>) {
        self.subscribers.push(subscriber);
    }
}
