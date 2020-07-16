use std::time::Duration;
use thiserror::Error;
use tokio::sync::broadcast::{channel, Sender};

use crate::db::model::SubscriptionId;

#[derive(Error, Debug)]
pub enum NotifierError {
    #[error("Timeout while waiting for events for subscription [{0}]")]
    Timeout(SubscriptionId),
    #[error("Unsubscribed [{0}]")]
    Unsubscribed(SubscriptionId),
    #[error("Channel closed while waiting for events for subscription [{0}]")]
    ChannelClosed(SubscriptionId),
}

/// Allows to listen to new incoming events and notify if event was generated.
#[derive(Clone)]
pub struct EventNotifier {
    sender: Sender<Notification<SubscriptionId>>,
}

#[derive(Clone)]
enum Notification<Type> {
    NewEvent(Type),
    StopEvents(Type),
}

impl EventNotifier {
    pub fn new() -> EventNotifier {
        // We will create receivers later, when someone needs it.
        let (sender, _receiver) = channel(100);
        EventNotifier { sender }
    }

    pub async fn notify(&self, subscription_id: &SubscriptionId) {
        let sender = self.sender.clone();
        let to_send = Notification::<SubscriptionId>::NewEvent(subscription_id.clone());
        // TODO: How to handle this error?
        let _ = sender.send(to_send);
    }

    pub async fn stop_notifying(&self, subscription_id: &SubscriptionId) {
        let sender = self.sender.clone();
        let to_send = Notification::<SubscriptionId>::StopEvents(subscription_id.clone());
        // TODO: How to handle this error?
        let _ = sender.send(to_send);
    }

    pub async fn wait_for_event(
        &self,
        subscription_id: &SubscriptionId,
    ) -> Result<(), NotifierError> {
        let mut receiver = self.sender.subscribe();
        while let Ok(value) = receiver.recv().await {
            match value {
                Notification::<SubscriptionId>::NewEvent(value) => {
                    if &value == subscription_id {
                        return Ok(());
                    }
                }
                Notification::<SubscriptionId>::StopEvents(subscription_id) => {
                    return Err(NotifierError::Unsubscribed(subscription_id));
                }
            }
        }
        Err(NotifierError::ChannelClosed(subscription_id.clone()))
    }

    pub async fn wait_for_event_with_timeout(
        &self,
        subscription_id: &SubscriptionId,
        timeout: Duration,
    ) -> Result<(), NotifierError> {
        let notifier = self.clone();
        match tokio::time::timeout(timeout, notifier.wait_for_event(subscription_id)).await {
            Err(_) => Err(NotifierError::Timeout(subscription_id.clone())),
            Ok(wait_result) => wait_result,
        }
    }
}
