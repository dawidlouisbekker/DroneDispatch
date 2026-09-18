//! Failed-attempt limits for passwords and passkey checks. In-process,
//! which is enough for one auth-service instance.

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct Limiter {
    max_failures: usize,
    window: Duration,
    failures: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Limiter {
    pub fn new(max_failures: usize, window: Duration) -> Self {
        Self {
            max_failures,
            window,
            failures: Mutex::default(),
        }
    }

    /// Whether another attempt is allowed for every key.
    pub fn allowed(&self, keys: &[&str]) -> bool {
        let now = Instant::now();
        let mut failures = self.failures.lock().expect("limiter lock poisoned");
        keys.iter().all(|key| {
            failures.get_mut(*key).is_none_or(|times| {
                while times
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > self.window)
                {
                    times.pop_front();
                }
                times.len() < self.max_failures
            })
        })
    }

    pub fn record_failure(&self, keys: &[&str]) {
        let now = Instant::now();
        let mut failures = self.failures.lock().expect("limiter lock poisoned");
        for key in keys {
            failures
                .entry((*key).to_owned())
                .or_default()
                .push_back(now);
        }
    }

    pub fn clear(&self, key: &str) {
        self.failures
            .lock()
            .expect("limiter lock poisoned")
            .remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_the_limit_until_cleared() {
        let limiter = Limiter::new(2, Duration::from_secs(60));
        assert!(limiter.allowed(&["a", "ip"]));
        limiter.record_failure(&["a", "ip"]);
        limiter.record_failure(&["a", "ip"]);
        assert!(!limiter.allowed(&["a"]));
        assert!(
            !limiter.allowed(&["b", "ip"]),
            "the shared IP key is also over the limit"
        );
        limiter.clear("a");
        assert!(limiter.allowed(&["a"]));
    }

    #[test]
    fn old_failures_expire() {
        let limiter = Limiter::new(1, Duration::from_millis(1));
        limiter.record_failure(&["a"]);
        std::thread::sleep(Duration::from_millis(5));
        assert!(limiter.allowed(&["a"]));
    }
}
