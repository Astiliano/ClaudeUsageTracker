use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use tokio::sync::Notify;

/// Spec 6.5: triggers reach the driver through a `Notify` per kind, never an
/// unbounded queue, so five Refresh clicks coalesce into at most one pending
/// trigger. `AccountChanged` additionally accumulates ids so enabling two
/// accounts in quick succession polls both.
#[derive(Debug, Default)]
pub struct Triggers {
    manual: Notify,
    startup: Notify,
    presence: Notify,
    changed: Notify,
    changed_ids: Mutex<HashSet<String>>,
}

impl Triggers {
    pub fn new() -> Triggers {
        Triggers::default()
    }

    pub fn manual(&self) {
        self.manual.notify_one();
    }

    pub fn startup(&self) {
        self.startup.notify_one();
    }

    /// Fired by the sampler on the zero-to-non-zero Claude process edge
    /// (spec §4.3). The sampler is the only caller.
    pub fn presence(&self) {
        self.presence.notify_one();
    }

    pub fn account_changed(&self, ids: Vec<String>) {
        {
            let mut set = self
                .changed_ids
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            set.extend(ids);
        }
        self.changed.notify_one();
    }

    /// Puts ids back into the set without arming the notification. Used by
    /// the driver when a drained `AccountChanged` decision was skipped: the
    /// ids must survive, but re-notifying would spin the select loop, so the
    /// driver's deferred flag re-arms them at the next flush point instead.
    pub fn defer_changed(&self, ids: Vec<String>) {
        let mut set = self
            .changed_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        set.extend(ids);
    }

    pub async fn notified_manual(&self) {
        self.manual.notified().await;
    }

    pub async fn notified_startup(&self) {
        self.startup.notified().await;
    }

    pub async fn notified_presence(&self) {
        self.presence.notified().await;
    }

    pub async fn notified_changed(&self) {
        self.changed.notified().await;
    }

    /// Drains every accumulated id into one decision.
    pub fn take_changed(&self) -> Vec<String> {
        let mut set = self
            .changed_ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        set.drain().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn five_manual_clicks_coalesce_into_one_pending_trigger() {
        let t = Arc::new(Triggers::new());
        for _ in 0..5 {
            t.manual();
        }
        // The first wait resolves immediately.
        tokio::time::timeout(Duration::from_millis(200), t.notified_manual())
            .await
            .expect("first notification");
        // There is no second pending notification.
        let second = tokio::time::timeout(Duration::from_millis(200), t.notified_manual()).await;
        assert!(second.is_err(), "clicks must coalesce, not queue");
    }

    #[tokio::test]
    async fn account_changed_ids_accumulate_and_drain_together() {
        let t = Arc::new(Triggers::new());
        t.account_changed(vec!["a".into()]);
        t.account_changed(vec!["b".into(), "a".into()]);

        tokio::time::timeout(Duration::from_millis(200), t.notified_changed())
            .await
            .expect("notification");

        let mut drained = t.take_changed();
        drained.sort();
        assert_eq!(drained, vec!["a".to_string(), "b".to_string()]);
        assert!(t.take_changed().is_empty(), "draining clears the set");
    }

    #[tokio::test]
    async fn deferring_ids_keeps_them_without_arming_the_notification() {
        let t = Arc::new(Triggers::new());
        t.account_changed(vec!["a".into()]);
        tokio::time::timeout(Duration::from_millis(200), t.notified_changed())
            .await
            .expect("notification");
        let drained = t.take_changed();
        assert_eq!(drained, vec!["a".to_string()]);

        t.defer_changed(drained);

        assert!(
            tokio::time::timeout(Duration::from_millis(200), t.notified_changed())
                .await
                .is_err(),
            "deferring must not re-arm the notification; that would spin the driver loop"
        );
        assert_eq!(t.take_changed(), vec!["a".to_string()], "the id survived");
    }

    #[tokio::test]
    async fn startup_and_manual_are_separate_channels() {
        let t = Arc::new(Triggers::new());
        t.startup();
        tokio::time::timeout(Duration::from_millis(200), t.notified_startup())
            .await
            .expect("startup notification");
        let manual = tokio::time::timeout(Duration::from_millis(200), t.notified_manual()).await;
        assert!(manual.is_err(), "startup must not fire the manual channel");
    }

    #[tokio::test]
    async fn presence_coalesces_like_manual() {
        let t = Arc::new(Triggers::new());
        t.presence();
        t.presence();

        tokio::time::timeout(Duration::from_millis(200), t.notified_presence())
            .await
            .expect("first notification");

        let second = tokio::time::timeout(Duration::from_millis(200), t.notified_presence()).await;
        assert!(second.is_err(), "presence wakes must coalesce, not queue");
    }

    #[tokio::test]
    async fn presence_is_its_own_channel() {
        let t = Arc::new(Triggers::new());
        t.presence();
        tokio::time::timeout(Duration::from_millis(200), t.notified_presence())
            .await
            .expect("presence notification");
        let manual = tokio::time::timeout(Duration::from_millis(200), t.notified_manual()).await;
        assert!(manual.is_err(), "presence must not fire the manual channel");
    }
}
