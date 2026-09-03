use std::time::{Instant, SystemTime, UNIX_EPOCH};

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiryDeadline {
    pub session_id: String,
    pub expires_at_monotonic_millis: i64,
    pub expires_at_wall_estimate_millis: i64,
}

#[derive(Clone, Copy)]
pub struct ExpiryClock {
    session_id: Uuid,
    process_start: Instant,
}

impl Default for ExpiryClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpiryClock {
    pub fn new() -> Self {
        Self {
            session_id: Uuid::new_v4(),
            process_start: Instant::now(),
        }
    }

    fn uptime_millis(&self) -> i64 {
        self.process_start.elapsed().as_millis() as i64
    }

    fn wall_now_millis() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the unix epoch")
            .as_millis() as i64
    }

    pub fn now_millis(&self) -> i64 {
        Self::wall_now_millis()
    }

    pub fn compute_deadline(&self, ttl_secs: u32) -> ExpiryDeadline {
        let ttl_millis = i64::from(ttl_secs) * 1000;
        ExpiryDeadline {
            session_id: self.session_id.to_string(),
            expires_at_monotonic_millis: self.uptime_millis() + ttl_millis,
            expires_at_wall_estimate_millis: Self::wall_now_millis() + ttl_millis,
        }
    }

    pub fn is_expired(&self, deadline: &ExpiryDeadline) -> bool {
        if deadline.session_id == self.session_id.to_string() {
            self.uptime_millis() >= deadline.expires_at_monotonic_millis
        } else {
            Self::wall_now_millis() >= deadline.expires_at_wall_estimate_millis
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_deadline_is_not_expired() {
        let clock = ExpiryClock::new();
        let deadline = clock.compute_deadline(600);
        assert!(!clock.is_expired(&deadline));
    }

    #[test]
    fn a_zero_ttl_deadline_is_immediately_expired() {
        let clock = ExpiryClock::new();
        let deadline = clock.compute_deadline(0);
        assert!(clock.is_expired(&deadline));
    }

    #[test]
    fn within_the_same_session_the_wall_clock_estimate_is_never_load_bearing() {
        let clock = ExpiryClock::new();
        let mut deadline = clock.compute_deadline(600);

        deadline.expires_at_wall_estimate_millis = 0;
        assert!(
            !clock.is_expired(&deadline),
            "a tampered/garbage wall estimate must not expire a fresh item while the monotonic session still matches"
        );

        deadline.expires_at_wall_estimate_millis = i64::MAX;
        assert!(
            !clock.is_expired(&deadline),
            "the wall estimate must not extend a deadline either, in either direction it is inert within the same session"
        );
    }

    #[test]
    fn a_different_session_id_falls_back_to_the_wall_clock_estimate_conservatively() {
        let original_clock = ExpiryClock::new();
        let deadline = original_clock.compute_deadline(600);

        let restarted_clock = ExpiryClock::new();
        assert_ne!(restarted_clock.session_id, original_clock.session_id);
        assert!(
            !restarted_clock.is_expired(&deadline),
            "a not-yet-due wall estimate should not expire the item across a restart"
        );

        let mut past_deadline = deadline.clone();
        past_deadline.expires_at_wall_estimate_millis = 1;
        assert!(
            restarted_clock.is_expired(&past_deadline),
            "once the monotonic session can no longer be trusted, an expired-per-wall-clock deadline must expire"
        );
    }

    #[test]
    fn session_ids_are_stable_within_a_clock_and_distinct_across_clocks() {
        let a = ExpiryClock::new();
        let b = ExpiryClock::new();
        let deadline_a1 = a.compute_deadline(60);
        let deadline_a2 = a.compute_deadline(120);
        assert_eq!(deadline_a1.session_id, deadline_a2.session_id);
        assert_ne!(deadline_a1.session_id, b.compute_deadline(60).session_id);
    }
}
