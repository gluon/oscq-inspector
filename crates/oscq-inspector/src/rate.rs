//! Send rate limiting for continuous widgets (PRD 9.3): at most one message per interval per
//! path while dragging; a value held back is sent when its slot comes, and the final value on
//! release is always sent.

use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const INTERVAL: Duration = Duration::from_millis(8);

#[derive(Debug)]
pub struct RateLimiter<T> {
    interval: Duration,
    last: HashMap<String, Instant>,
    deferred: HashMap<String, T>,
}

impl<T> RateLimiter<T> {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: HashMap::new(),
            deferred: HashMap::new(),
        }
    }

    /// A value from a drag. Returns it if it may go now; otherwise keeps it as the latest value
    /// for that path, replacing any older held value.
    pub fn offer(&mut self, now: Instant, key: &str, item: T) -> Option<T> {
        let due = self
            .last
            .get(key)
            .is_none_or(|t| now.saturating_duration_since(*t) >= self.interval);
        if due {
            self.last.insert(key.to_owned(), now);
            self.deferred.remove(key);
            Some(item)
        } else {
            self.deferred.insert(key.to_owned(), item);
            None
        }
    }

    /// The final value on release: always sent, any held value for the path dropped.
    pub fn finish(&mut self, now: Instant, key: &str, item: T) -> T {
        self.deferred.remove(key);
        self.last.insert(key.to_owned(), now);
        item
    }

    /// Held values whose slot has come.
    pub fn due(&mut self, now: Instant) -> Vec<T> {
        let ready: Vec<String> = self
            .deferred
            .keys()
            .filter(|k| {
                self.last
                    .get(*k)
                    .is_none_or(|t| now.saturating_duration_since(*t) >= self.interval)
            })
            .cloned()
            .collect();
        ready
            .into_iter()
            .filter_map(|k| {
                self.last.insert(k.clone(), now);
                self.deferred.remove(&k)
            })
            .collect()
    }

    /// Time until the next held value is due, to schedule exactly one repaint. `None` when
    /// nothing is held, so an idle UI never repaints.
    pub fn next_due(&self, now: Instant) -> Option<Duration> {
        self.deferred
            .keys()
            .filter_map(|k| self.last.get(k))
            .map(|t| (*t + self.interval).saturating_duration_since(now))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_per_interval_latest_kept_final_always() {
        let mut r = RateLimiter::new(INTERVAL);
        let t0 = Instant::now();
        assert_eq!(r.offer(t0, "/a", 1), Some(1));
        assert_eq!(r.offer(t0 + Duration::from_millis(2), "/a", 2), None);
        assert_eq!(r.offer(t0 + Duration::from_millis(4), "/a", 3), None);
        assert_eq!(
            r.offer(t0 + Duration::from_millis(1), "/b", 9),
            Some(9),
            "per path"
        );
        assert_eq!(
            r.next_due(t0 + Duration::from_millis(4)),
            Some(Duration::from_millis(4))
        );
        assert_eq!(r.due(t0 + Duration::from_millis(5)), Vec::<i32>::new());
        assert_eq!(
            r.due(t0 + Duration::from_millis(8)),
            vec![3],
            "latest held value"
        );
        assert_eq!(r.next_due(t0 + Duration::from_millis(8)), None, "idle");
        assert_eq!(r.offer(t0 + Duration::from_millis(9), "/a", 4), None);
        assert_eq!(r.finish(t0 + Duration::from_millis(10), "/a", 5), 5);
        assert_eq!(
            r.due(t0 + Duration::from_millis(100)),
            Vec::<i32>::new(),
            "held dropped"
        );
    }
}
