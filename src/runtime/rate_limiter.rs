//! Token-bucket rate limiter.

use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// A simple token-bucket rate limiter with separate limits for general and
/// search requests.
pub struct RateLimiter {
    general: Mutex<Bucket>,
    search: Mutex<Bucket>,
}

struct Bucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl Bucket {
    fn new(rps: f64) -> Self {
        Self {
            tokens: rps,
            max_tokens: rps,
            refill_rate: rps,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }

    /// Returns `None` if a token is available immediately, or `Some(delay)` if
    /// the caller must wait.
    fn try_acquire(&mut self) -> Option<Duration> {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            None
        } else {
            let deficit = 1.0 - self.tokens;
            let wait = deficit / self.refill_rate;
            Some(Duration::from_secs_f64(wait))
        }
    }
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// * `rps_general` — requests per second for normal endpoints.
    /// * `rps_search` — requests per second for search endpoints.
    pub fn new(rps_general: f64, rps_search: f64) -> Self {
        Self {
            general: Mutex::new(Bucket::new(rps_general)),
            search: Mutex::new(Bucket::new(rps_search)),
        }
    }

    /// Wait until a request is allowed.
    ///
    /// If `is_search` is true, the stricter search bucket is used **in
    /// addition** to the general bucket.
    pub async fn wait(&self, is_search: bool) {
        // Always consume from the general bucket.
        loop {
            let delay = {
                let mut bucket = self.general.lock().await;
                bucket.try_acquire()
            };
            match delay {
                None => break,
                Some(d) => tokio::time::sleep(d).await,
            }
        }

        if is_search {
            loop {
                let delay = {
                    let mut bucket = self.search.lock().await;
                    bucket.try_acquire()
                };
                match delay {
                    None => break,
                    Some(d) => tokio::time::sleep(d).await,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_request_is_immediate() {
        let rl = RateLimiter::new(3.0, 1.0);
        let start = Instant::now();
        rl.wait(false).await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
