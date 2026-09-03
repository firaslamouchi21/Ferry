use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const REACHABLE_WITHIN_MILLIS: i64 = 90_000;

#[derive(Default)]
pub struct Presence {
    last_seen: Mutex<HashMap<String, i64>>,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Presence {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seen(&self, peer_id: &str) {
        self.seen_at(peer_id, now_millis());
    }

    pub fn seen_at(&self, peer_id: &str, at_millis: i64) {
        self.last_seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(peer_id.to_string(), at_millis);
    }

    pub fn last_seen(&self, peer_id: &str) -> Option<i64> {
        self.last_seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(peer_id).copied()
    }

    pub fn is_reachable(&self, peer_id: &str) -> bool {
        self.is_reachable_at(peer_id, now_millis())
    }

    pub fn is_reachable_at(&self, peer_id: &str, now: i64) -> bool {
        self.last_seen(peer_id)
            .map(|seen| now.saturating_sub(seen) <= REACHABLE_WITHIN_MILLIS)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unseen_peer_is_not_reachable_and_has_no_last_seen() {
        let p = Presence::new();
        assert_eq!(p.last_seen("peer-1"), None);
        assert!(!p.is_reachable("peer-1"));
    }

    #[test]
    fn a_recently_seen_peer_is_reachable_and_an_old_one_is_not() {
        let p = Presence::new();
        p.seen_at("fresh", 100_000);
        p.seen_at("stale", 100_000);

        assert!(p.is_reachable_at("fresh", 100_000 + REACHABLE_WITHIN_MILLIS));
        assert!(!p.is_reachable_at("stale", 100_000 + REACHABLE_WITHIN_MILLIS + 1));
        assert_eq!(p.last_seen("fresh"), Some(100_000));
    }

    #[test]
    fn seen_updates_the_timestamp() {
        let p = Presence::new();
        p.seen_at("peer-1", 1_000);
        p.seen_at("peer-1", 5_000);
        assert_eq!(p.last_seen("peer-1"), Some(5_000));
    }
}
