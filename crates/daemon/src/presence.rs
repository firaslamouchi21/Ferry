use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const REACHABLE_WITHIN_MILLIS: i64 = 90_000;

pub const FAILURES_BEFORE_OFFLINE: u32 = 2;

#[derive(Debug, Clone, Copy)]
struct Contact {
    last_seen: Option<i64>,
    failures: u32,
    was_reachable: bool,
}

#[derive(Default)]
pub struct Presence {
    contacts: Mutex<HashMap<String, Contact>>,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Contact {
    fn is_reachable_at(&self, now: i64) -> bool {
        self.failures < FAILURES_BEFORE_OFFLINE
            && self
                .last_seen
                .map(|seen| (0..=REACHABLE_WITHIN_MILLIS).contains(&now.saturating_sub(seen)))
                .unwrap_or(false)
    }
}

impl Presence {
    pub fn new() -> Self {
        Self::default()
    }

    fn update(&self, peer_id: &str, now: i64, change: impl FnOnce(&mut Contact)) -> bool {
        let mut contacts = self.contacts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let contact = contacts.entry(peer_id.to_string()).or_insert(Contact {
            last_seen: None,
            failures: 0,
            was_reachable: false,
        });
        change(contact);
        let reachable = contact.is_reachable_at(now);
        std::mem::replace(&mut contact.was_reachable, reachable) != reachable
    }

    pub fn seen(&self, peer_id: &str) -> bool {
        self.seen_at(peer_id, now_millis())
    }

    pub fn seen_at(&self, peer_id: &str, at_millis: i64) -> bool {
        self.update(peer_id, at_millis, |contact| {
            contact.last_seen = Some(at_millis);
            contact.failures = 0;
        })
    }

    pub fn contact_failed(&self, peer_id: &str) -> bool {
        self.contact_failed_at(peer_id, now_millis())
    }

    pub fn contact_failed_at(&self, peer_id: &str, at_millis: i64) -> bool {
        self.update(peer_id, at_millis, |contact| {
            contact.failures = contact.failures.saturating_add(1);
        })
    }

    pub fn contact_is_due(&self, peer_id: &str, interval_millis: i64) -> bool {
        self.contact_is_due_at(peer_id, interval_millis, now_millis())
    }

    pub fn contact_is_due_at(&self, peer_id: &str, interval_millis: i64, now: i64) -> bool {
        self.contacts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(peer_id)
            .map(|contact| {
                contact.failures > 0
                    || contact
                        .last_seen
                        .map(|seen| !(0..interval_millis).contains(&now.saturating_sub(seen)))
                        .unwrap_or(true)
            })
            .unwrap_or(true)
    }

    pub fn last_seen(&self, peer_id: &str) -> Option<i64> {
        self.contacts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(peer_id)
            .and_then(|contact| contact.last_seen)
    }

    pub fn is_reachable(&self, peer_id: &str) -> bool {
        self.is_reachable_at(peer_id, now_millis())
    }

    pub fn is_reachable_at(&self, peer_id: &str, now: i64) -> bool {
        self.contacts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(peer_id)
            .map(|contact| contact.is_reachable_at(now))
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

    #[test]
    fn repeated_contact_failures_turn_a_peer_offline_at_once_and_one_success_restores_it() {
        let p = Presence::new();
        assert!(p.seen_at("peer-1", 1_000), "first contact is a visible change");
        assert!(!p.seen_at("peer-1", 2_000), "a repeat contact is not");
        assert!(!p.contact_failed_at("peer-1", 3_000), "one lost probe must not flap the status");
        assert!(p.is_reachable_at("peer-1", 3_000));
        assert!(p.contact_failed_at("peer-1", 4_000));
        assert!(!p.is_reachable_at("peer-1", 4_000));
        assert_eq!(p.last_seen("peer-1"), Some(2_000));
        assert!(p.seen_at("peer-1", 5_000));
        assert!(p.is_reachable_at("peer-1", 5_000));
    }

    #[test]
    fn contact_is_due_for_unknown_stale_and_failing_peers_only() {
        let p = Presence::new();
        assert!(p.contact_is_due_at("peer-1", 20_000, 0));
        p.seen_at("peer-1", 1_000);
        assert!(!p.contact_is_due_at("peer-1", 20_000, 20_000));
        assert!(p.contact_is_due_at("peer-1", 20_000, 21_000));
        p.seen_at("peer-1", 30_000);
        p.contact_failed_at("peer-1", 31_000);
        assert!(p.contact_is_due_at("peer-1", 20_000, 32_000));
    }

    #[test]
    fn a_clock_that_moved_backwards_does_not_leave_a_peer_online_forever() {
        let p = Presence::new();
        p.seen_at("peer-1", 1_000_000);
        assert!(!p.is_reachable_at("peer-1", 1_000));
    }
}
