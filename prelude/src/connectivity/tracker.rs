use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::{
    sync::watch,
    task::{self, JoinHandle},
};

/// Tracks the latest known internet connectivity state.
///
/// New trackers start offline until updated by the caller.
/// Clones share the same underlying connectivity state and publish updates to
/// the same subscribers.
#[derive(Clone)]
pub struct ConnectivityTracker {
    tx: watch::Sender<bool>,
    rx: watch::Receiver<bool>,
}

impl Default for ConnectivityTracker {
    fn default() -> Self {
        let (tx, rx) = watch::channel(true);

        Self { tx, rx }
    }
}

impl ConnectivityTracker {
    /// Returns the latest connectivity state reported through [`Self::update`].
    pub fn is_online(&self) -> bool {
        *self.rx.borrow()
    }

    /// Publishes a new connectivity state.
    ///
    /// Stability trackers created with [`Self::track_stability`] observe these
    /// updates asynchronously.
    pub fn update(&self, is_online: bool) {
        let _ = self.tx.send(is_online);
    }

    /// Starts tracking whether connectivity remains stable from this point on.
    ///
    /// The returned handle reports unstable after it observes an offline update.
    /// Dropping the handle stops the background tracking task.
    pub fn track_stability(&self) -> ConnectivityStability {
        ConnectivityStability::new(self.tx.subscribe())
    }
}

/// Tracks whether connectivity has remained online for the lifetime of this handle.
pub struct ConnectivityStability {
    stable_connection: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl Drop for ConnectivityStability {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl ConnectivityStability {
    fn new(mut is_online_rx: watch::Receiver<bool>) -> Self {
        let stable_connection = Arc::new(AtomicBool::new(*is_online_rx.borrow()));
        let sc = stable_connection.clone();

        let handle = task::spawn(async move {
            while let Ok(()) = is_online_rx.changed().await {
                if !*is_online_rx.borrow() {
                    sc.store(false, Ordering::Release);
                }
            }
        });

        Self {
            stable_connection,
            handle,
        }
    }

    /// Returns true if connectivity has remained online since this instance was created.
    pub fn is_stable(&self) -> bool {
        self.stable_connection.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectivityStability, ConnectivityTracker};
    use std::time::Duration;
    use tokio::time::timeout;

    async fn wait_until_unstable(stability: &ConnectivityStability) {
        timeout(Duration::from_secs(1), async {
            while stability.is_stable() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("stability tracker did not observe offline update");
    }

    #[test]
    fn default_tracker_starts_online() {
        // Arrange
        let tracker = ConnectivityTracker::default();

        // Act
        let is_online = tracker.is_online();

        // Assert
        assert!(is_online);
    }

    #[test]
    fn update_sets_latest_online_state() {
        // Arrange
        let tracker = ConnectivityTracker::default();

        // Act
        tracker.update(true);

        // Assert
        assert!(tracker.is_online());

        // Act
        tracker.update(false);

        // Assert
        assert!(!tracker.is_online());
    }

    #[tokio::test]
    async fn stability_starts_with_current_state() {
        // Arrange
        let tracker = ConnectivityTracker::default();
        tracker.update(true);

        // Act
        let stability = tracker.track_stability();

        // Assert
        assert!(stability.is_stable());
    }

    #[tokio::test]
    async fn stability_starts_unstable_when_created_while_offline() {
        // Arrange
        let tracker = ConnectivityTracker::default();
        tracker.update(false);

        // Act
        let stability = tracker.track_stability();

        // Assert
        assert!(!stability.is_stable());
    }

    #[tokio::test]
    async fn stability_becomes_unstable_after_offline_update() {
        // Arrange
        let tracker = ConnectivityTracker::default();
        tracker.update(true);
        let stability = tracker.track_stability();

        // Act
        tracker.update(false);
        wait_until_unstable(&stability).await;

        // Assert
        assert!(!stability.is_stable());
    }

    #[tokio::test]
    async fn stability_does_not_recover_after_coming_back_online() {
        // Arrange
        let tracker = ConnectivityTracker::default();
        tracker.update(true);
        let stability = tracker.track_stability();

        // Act
        tracker.update(false);
        wait_until_unstable(&stability).await;
        tracker.update(true);

        // Assert
        assert!(!stability.is_stable());
    }

    #[tokio::test]
    async fn separate_stability_handles_track_from_their_creation_point() {
        // Arrange
        let tracker = ConnectivityTracker::default();
        tracker.update(true);
        let first_stability = tracker.track_stability();

        // Act
        tracker.update(false);
        wait_until_unstable(&first_stability).await;
        tracker.update(true);
        let second_stability = tracker.track_stability();

        // Assert
        assert!(!first_stability.is_stable());
        assert!(second_stability.is_stable());
    }

    #[tokio::test]
    async fn cloned_trackers_share_stability_updates() {
        // Arrange
        let tracker = ConnectivityTracker::default();
        tracker.update(true);
        let cloned_tracker = tracker.clone();
        let stability = tracker.track_stability();

        // Act
        cloned_tracker.update(false);
        wait_until_unstable(&stability).await;

        // Assert
        assert!(!stability.is_stable());
    }
}
