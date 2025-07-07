use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time;

use crate::negotiation::LastChange;

/// Manages synchronization for Offers to handle race conditions between
/// offer publication and incoming requests.
#[derive(Clone)]
pub struct OfferSync {
    /// Set of timestamps representing reserved placeholders for offers being published
    placeholders: Arc<Mutex<HashSet<Instant>>>,
    /// Last change notifier for notifications
    last_change: LastChange,
}

impl OfferSync {
    /// Creates a new OfferSync instance
    pub fn new(last_change: LastChange) -> Self {
        Self {
            placeholders: Arc::new(Mutex::new(HashSet::new())),
            last_change,
        }
    }

    /// Reserves a placeholder for a new offer being published.
    /// Returns a guard that will automatically remove the placeholder when dropped.
    pub fn reserve_placeholder(&self) -> OfferPlaceholderGuard {
        let instant = Instant::now();
        {
            let mut placeholders = self.placeholders.lock().unwrap();
            placeholders.insert(instant);
        }

        OfferPlaceholderGuard {
            instant,
            placeholders: self.placeholders.clone(),
        }
    }

    /// Gets a snapshot of current placeholders for use in request handlers.
    /// This allows the handler to remember which offers are being published
    /// and should be available soon.
    pub fn get_placeholder_snapshot(&self) -> OfferSyncContext {
        let initial_placeholders = {
            let placeholders = self.placeholders.lock().unwrap();
            placeholders.clone()
        };

        OfferSyncContext {
            initial_placeholders,
            sync: self.clone(),
        }
    }

    /// Waits for notifications and checks if the initial placeholders are still needed.
    /// Returns true if all initial placeholders are still present, false otherwise.
    pub async fn wait_for_placeholders_offers(
        &self,
        initial_placeholders: &HashSet<Instant>,
        timeout: Duration,
    ) -> bool {
        let mut wait = self.last_change.subscribe();
        let mut remaining_timeout = timeout;

        loop {
            // Check if all initial placeholders are still present
            let current_placeholders = self.placeholders.lock().unwrap().clone();
            let any_still_present = initial_placeholders
                .iter()
                .any(|instant| current_placeholders.contains(instant));

            if !any_still_present {
                return true;
            }

            // Wait for notification or timeout
            let wait_start = Instant::now();
            match time::timeout(remaining_timeout, wait.changed()).await {
                Ok(Ok(_)) => {
                    // Notification received, check again
                    // Decrement timeout by actual elapsed time
                    let wait_elapsed = wait_start.elapsed();
                    remaining_timeout = remaining_timeout.saturating_sub(wait_elapsed);
                    if remaining_timeout.is_zero() {
                        return false;
                    }
                    continue;
                }
                Ok(Err(_)) => {
                    // Channel closed, assume placeholders are no longer needed
                    return false;
                }
                Err(_) => {
                    // Timeout reached, assume placeholders are no longer needed
                    return false;
                }
            }
        }
    }

    /// Checks if there are any active placeholders
    pub fn has_active_placeholders(&self) -> bool {
        let placeholders = self.placeholders.lock().unwrap();
        !placeholders.is_empty()
    }

    /// Gets the count of active placeholders
    pub fn placeholder_count(&self) -> usize {
        let placeholders = self.placeholders.lock().unwrap();
        placeholders.len()
    }
}

/// Guard that automatically removes a placeholder when dropped.
/// This ensures that placeholders are properly cleaned up even if
/// the offer publication fails or panics.
pub struct OfferPlaceholderGuard {
    instant: Instant,
    placeholders: Arc<Mutex<HashSet<Instant>>>,
}

impl Drop for OfferPlaceholderGuard {
    fn drop(&mut self) {
        if let Ok(mut placeholders) = self.placeholders.lock() {
            placeholders.remove(&self.instant);
        }
    }
}

/// Context for handling offer synchronization in request handlers.
/// This struct encapsulates the logic for checking if an offer might be
/// available soon based on active placeholders.
pub struct OfferSyncContext {
    /// Snapshot of placeholders taken when the request started
    initial_placeholders: HashSet<Instant>,
    /// Reference to the sync manager
    sync: OfferSync,
}

impl OfferSyncContext {
    /// Creates a new context for handling offer synchronization
    pub fn new(sync: OfferSync) -> Self {
        sync.get_placeholder_snapshot()
    }

    /// Waits for notifications and checks if the offer might become available.
    /// Returns true if we should continue waiting, false if we should stop.
    pub async fn wait_for_offer_availability(&self, timeout: Duration) -> bool {
        self.sync
            .wait_for_placeholders_offers(&self.initial_placeholders, timeout)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_offer_sync_sync_mechanism() {
        let last_change = LastChange::new();
        let sync = OfferSync::new(last_change);

        // Initially no placeholders
        assert_eq!(sync.placeholder_count(), 0);

        // Reserve a placeholder
        let guard = sync.reserve_placeholder();
        assert_eq!(sync.placeholder_count(), 1);

        // Take snapshot
        let snapshot = sync.get_placeholder_snapshot();
        assert_eq!(snapshot.initial_placeholders.len(), 1);

        // Create context with placeholder
        let context = OfferSyncContext::new(sync.clone());
        assert!(!context.initial_placeholders.is_empty());
        assert_eq!(context.initial_placeholders.len(), 1);

        // Drop the guard
        drop(guard);

        // Placeholder should be removed immediately
        assert_eq!(sync.placeholder_count(), 0);

        // Snapshot should still have the placeholder (it's a snapshot)
        assert_eq!(snapshot.initial_placeholders.len(), 1);

        // Create context without placeholder
        let context = OfferSyncContext::new(sync.clone());
        assert!(context.initial_placeholders.is_empty());
        assert_eq!(context.initial_placeholders.len(), 0);
    }

    #[tokio::test]
    async fn test_offer_sync_waiting_with_no_offers() {
        let last_change = LastChange::new();
        let sync = OfferSync::new(last_change);

        // Create context with no placeholders
        let context = OfferSyncContext::new(sync.clone());
        assert!(context.initial_placeholders.is_empty());

        // Wait for offers with a short timeout
        let start = Instant::now();
        let result = context
            .wait_for_offer_availability(Duration::from_secs(10))
            .await;
        let elapsed = start.elapsed();

        // Should return true immediately since no placeholders exist
        assert!(result);
        // Should not have waited the full timeout
        assert!(elapsed < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_offer_sync_timeout_behavior() {
        let last_change = LastChange::new();
        let sync = OfferSync::new(last_change);

        // Reserve a placeholder
        let _guard = sync.reserve_placeholder();
        assert_eq!(sync.placeholder_count(), 1);

        // Create context with placeholder
        let context = OfferSyncContext::new(sync.clone());
        assert!(!context.initial_placeholders.is_empty());

        // Wait for offers with a short timeout
        let start = Instant::now();
        let result = context
            .wait_for_offer_availability(Duration::from_millis(200))
            .await;
        let elapsed = start.elapsed();

        // Should timeout and return false
        assert!(!result);
        // Should have waited approximately the timeout duration
        assert!(elapsed >= Duration::from_millis(200));
    }

    #[tokio::test]
    async fn test_offer_sync_multiple_offers() {
        let last_change = LastChange::new();
        let sync = OfferSync::new(last_change);

        // Reserve multiple placeholders
        let guard1 = sync.reserve_placeholder();
        let guard2 = sync.reserve_placeholder();
        let guard3 = sync.reserve_placeholder();

        assert_eq!(sync.placeholder_count(), 3);

        // Create context with multiple placeholders
        let context = OfferSyncContext::new(sync.clone());
        assert_eq!(context.initial_placeholders.len(), 3);

        // Drop one guard
        drop(guard1);
        assert_eq!(sync.placeholder_count(), 2);

        // Wait for offers - should return false since one placeholder was removed
        let result = context
            .wait_for_offer_availability(Duration::from_millis(100))
            .await;
        assert!(!result);

        // Drop remaining guards
        drop(guard2);
        drop(guard3);
        assert_eq!(sync.placeholder_count(), 0);

        // Check waiting afterwards - should return true immediately since no placeholders
        let start = Instant::now();
        let result_after = context
            .wait_for_offer_availability(Duration::from_millis(100))
            .await;
        let elapsed = start.elapsed();
        assert!(result_after);
        // Should return immediately, not wait for timeout
        assert!(elapsed < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_offer_sync_offer_becomes_available() {
        let last_change = LastChange::new();
        let sync = OfferSync::new(last_change);

        // Reserve a placeholder
        let guard = sync.reserve_placeholder();
        assert_eq!(sync.placeholder_count(), 1);

        // Create context with placeholder
        let context = OfferSyncContext::new(sync.clone());
        assert!(!context.initial_placeholders.is_empty());

        // Spawn a task to drop the guard after a short delay
        let sync_clone = sync.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(guard);
        });

        // Wait for offers - should return false when guard is dropped
        let result = context
            .wait_for_offer_availability(Duration::from_millis(200))
            .await;
        assert!(!result);

        // Verify placeholder was removed
        assert_eq!(sync_clone.placeholder_count(), 0);
    }

    #[tokio::test]
    async fn test_offer_sync_partial_availability_timeout() {
        let last_change = LastChange::new();
        let sync = OfferSync::new(last_change);

        // Reserve multiple placeholders
        let guard1 = sync.reserve_placeholder();
        let guard2 = sync.reserve_placeholder();
        assert_eq!(sync.placeholder_count(), 2);

        // Create context with multiple placeholders
        let context = OfferSyncContext::new(sync.clone());
        assert_eq!(context.initial_placeholders.len(), 2);

        // Drop one guard (one offer becomes available)
        drop(guard1);
        assert_eq!(sync.placeholder_count(), 1);

        // Wait for offers - should timeout since one placeholder still exists
        let start = Instant::now();
        let result = context
            .wait_for_offer_availability(Duration::from_millis(200))
            .await;
        let elapsed = start.elapsed();

        // Should timeout and return false since one placeholder still exists
        assert!(!result);
        // Should have waited approximately the timeout duration
        assert!(elapsed >= Duration::from_millis(200));

        // Drop the remaining guard
        drop(guard2);
        assert_eq!(sync.placeholder_count(), 0);

        // Check waiting afterwards - should return true immediately since no placeholders
        let start_after = Instant::now();
        let result_after = context
            .wait_for_offer_availability(Duration::from_millis(100))
            .await;
        let elapsed_after = start_after.elapsed();
        assert!(result_after);
        // Should return immediately, not wait for timeout
        assert!(elapsed_after < Duration::from_millis(50));
    }
}
