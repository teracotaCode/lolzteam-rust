//! Retry logic with exponential backoff and jitter.

use crate::runtime::errors::LolzteamError;
use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

/// Configuration for the retry policy.
pub struct RetryConfig {
    /// Maximum number of retry attempts (0 means no retries).
    pub max_retries: u32,
    /// Base delay between retries (before exponential increase).
    pub base_delay: Duration,
    /// Maximum delay cap.
    pub max_delay: Duration,
    /// HTTP status codes that should be retried.
    pub retry_statuses: HashSet<u16>,
    /// Optional callback invoked before each retry.
    pub on_retry: Option<Box<dyn Fn(u32, Duration, &LolzteamError) + Send + Sync>>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            retry_statuses: [408, 429, 500, 502, 503, 504].into_iter().collect(),
            on_retry: None,
        }
    }
}

impl std::fmt::Debug for RetryConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetryConfig")
            .field("max_retries", &self.max_retries)
            .field("base_delay", &self.base_delay)
            .field("max_delay", &self.max_delay)
            .field("retry_statuses", &self.retry_statuses)
            .field("on_retry", &self.on_retry.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

/// Compute the delay for the given attempt using exponential backoff + jitter.
fn compute_delay(attempt: u32, base: Duration, max: Duration, retry_after: Option<f64>) -> Duration {
    // If the server told us when to retry, honour it (clamped).
    if let Some(secs) = retry_after {
        let d = Duration::from_secs_f64(secs);
        return d.min(max);
    }

    // Exponential backoff: base * 2^attempt
    let exp = base.saturating_mul(1u32 << attempt.min(10));

    // Add jitter: uniform in [0, exp]
    let jitter_ms = if exp.as_millis() > 0 {
        fastrand::u64(0..exp.as_millis() as u64)
    } else {
        0
    };
    let delay = exp + Duration::from_millis(jitter_ms);
    delay.min(max)
}

/// Execute a closure with retry logic.
///
/// The closure `f` is called repeatedly until it succeeds, returns a
/// non-retryable error, or the maximum number of retries is exhausted.
///
/// `f` is an `FnMut` that returns a `Future` — it will be called once per
/// attempt.
pub async fn execute_with_retry<F, Fut, T>(
    mut f: F,
    config: &RetryConfig,
) -> Result<T, LolzteamError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, LolzteamError>>,
{
    let mut attempt: u32 = 0;

    loop {
        match f().await {
            Ok(val) => return Ok(val),
            Err(err) => {
                if attempt >= config.max_retries || !err.is_retryable() {
                    if attempt > 0 {
                        return Err(LolzteamError::RetryExhausted {
                            attempts: attempt + 1,
                            last_error: Box::new(err),
                        });
                    }
                    return Err(err);
                }

                let delay = compute_delay(
                    attempt,
                    config.base_delay,
                    config.max_delay,
                    err.retry_after(),
                );

                if let Some(ref cb) = config.on_retry {
                    cb(attempt + 1, delay, &err);
                }

                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn succeeds_immediately() {
        let result = execute_with_retry(|| async { Ok::<_, LolzteamError>(42) }, &RetryConfig::default()).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn retries_then_succeeds() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let config = RetryConfig {
            max_retries: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            ..Default::default()
        };

        let result = execute_with_retry(
            move || {
                let c = c.clone();
                async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        Err(LolzteamError::Server {
                            status: 503,
                            body: "unavailable".into(),
                        })
                    } else {
                        Ok("ok")
                    }
                }
            },
            &config,
        )
        .await;

        assert_eq!(result.unwrap(), "ok");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn non_retryable_fails_immediately() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();

        let result = execute_with_retry(
            move || {
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>(LolzteamError::Auth {
                        body: "bad token".into(),
                    })
                }
            },
            &RetryConfig::default(),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
