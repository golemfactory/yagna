use actix::prelude::*;
use anyhow::Result;


/// Subscribe to process signals.
#[derive(Message)]
#[rtype(result = "()")]
pub struct Subscribe<MessageType>(pub Recipient<MessageType>)
    where MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
          MessageType::Result: std::marker::Send + std::marker::Sync;

/// Actor that provides signal subscriptions
pub struct SignalSlot<MessageType>
    where MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
          MessageType::Result: std::marker::Send + std::marker::Sync
{
    subscribers: Vec<Recipient<MessageType>>,
}

impl<MessageType> SignalSlot<MessageType>
    where MessageType: Message + std::marker::Send + std::marker::Sync + std::clone::Clone,
          MessageType::Result: std::marker::Send + std::marker::Sync
{
    pub fn new() -> SignalSlot<MessageType> {
        SignalSlot::<MessageType>{subscribers: vec![]}
    }

    /// Send signal to all subscribers
    pub fn send_signal(&mut self, message: MessageType) -> Result<()> {
        for subscriber in &self.subscribers {
            subscriber.do_send(message.clone());
        }
        Ok(())
    }

    pub fn subscribe(&mut self, subscriber: Recipient<MessageType>) {
        self.subscribers.push(subscriber);
    }
}
