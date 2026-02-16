//! Retry strategy abstraction for resilient operations.
//!
//! This module provides traits and implementations for retry logic with
//! configurable backoff strategies.

use std::time::Duration;

use crate::config::retry as retry_config;

/// Trait for defining retry strategies.
///
/// Implementations define how many attempts to make and how long to wait
/// between each attempt.
pub trait RetryStrategy {
    /// Maximum number of retry attempts.
    fn max_attempts(&self) -> u32;

    /// Delay before the given attempt (0-indexed).
    ///
    /// Returns `None` if no delay should be applied (e.g., for the first attempt).
    fn delay_for_attempt(&self, attempt: u32) -> Option<Duration>;

    /// Execute a function with retry logic.
    ///
    /// Calls `f` up to `max_attempts()` times, sleeping `delay_for_attempt()`
    /// between each failed attempt.
    fn execute<T, E, F>(&self, mut f: F) -> Result<T, E>
    where
        F: FnMut(u32) -> Result<T, E>,
    {
        let max = self.max_attempts();
        let mut last_error: Option<E> = None;

        for attempt in 0..max {
            match f(attempt) {
                Ok(value) => return Ok(value),
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < max
                        && let Some(delay) = self.delay_for_attempt(attempt)
                    {
                        std::thread::sleep(delay);
                    }
                }
            }
        }

        Err(last_error.expect("at least one retry attempt"))
    }
}

/// Exponential backoff retry strategy.
///
/// Uses the configured delays from `config::retry`.
#[derive(Debug, Clone, Default)]
pub struct ExponentialBackoff;

impl ExponentialBackoff {
    /// Create a new exponential backoff strategy.
    pub fn new() -> Self {
        Self
    }
}

impl RetryStrategy for ExponentialBackoff {
    fn max_attempts(&self) -> u32 {
        retry_config::MAX_READ_RETRIES
    }

    fn delay_for_attempt(&self, attempt: u32) -> Option<Duration> {
        retry_config::RETRY_DELAYS_MS
            .get(attempt as usize)
            .map(|&ms| Duration::from_millis(ms))
    }
}

/// Fixed delay retry strategy.
///
/// Waits a constant duration between each attempt.
#[derive(Debug, Clone)]
pub struct FixedDelay {
    max_attempts: u32,
    delay: Duration,
}

impl FixedDelay {
    /// Create a new fixed delay strategy.
    pub fn new(max_attempts: u32, delay: Duration) -> Self {
        Self {
            max_attempts,
            delay,
        }
    }
}

impl RetryStrategy for FixedDelay {
    fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    fn delay_for_attempt(&self, _attempt: u32) -> Option<Duration> {
        Some(self.delay)
    }
}

/// No retry strategy - attempt once and return the result.
#[derive(Debug, Clone, Default)]
pub struct NoRetry;

impl NoRetry {
    /// Create a no-retry strategy.
    pub fn new() -> Self {
        Self
    }
}

impl RetryStrategy for NoRetry {
    fn max_attempts(&self) -> u32 {
        1
    }

    fn delay_for_attempt(&self, _attempt: u32) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_backoff_max_attempts() {
        let strategy = ExponentialBackoff::new();
        assert_eq!(strategy.max_attempts(), 5);
    }

    #[test]
    fn test_exponential_backoff_delays() {
        let strategy = ExponentialBackoff::new();

        assert_eq!(
            strategy.delay_for_attempt(0),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            strategy.delay_for_attempt(1),
            Some(Duration::from_millis(200))
        );
        assert_eq!(
            strategy.delay_for_attempt(2),
            Some(Duration::from_millis(400))
        );
        assert_eq!(
            strategy.delay_for_attempt(3),
            Some(Duration::from_millis(800))
        );
        assert_eq!(
            strategy.delay_for_attempt(4),
            Some(Duration::from_millis(1600))
        );
        assert_eq!(strategy.delay_for_attempt(5), None);
    }

    #[test]
    fn test_fixed_delay() {
        let strategy = FixedDelay::new(3, Duration::from_millis(50));

        assert_eq!(strategy.max_attempts(), 3);
        assert_eq!(
            strategy.delay_for_attempt(0),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            strategy.delay_for_attempt(1),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            strategy.delay_for_attempt(2),
            Some(Duration::from_millis(50))
        );
    }

    #[test]
    fn test_no_retry() {
        let strategy = NoRetry::new();

        assert_eq!(strategy.max_attempts(), 1);
        assert_eq!(strategy.delay_for_attempt(0), None);
    }

    #[test]
    fn test_execute_success_first_try() {
        let strategy = ExponentialBackoff::new();
        let result: Result<i32, &str> = strategy.execute(|_| Ok(42));
        assert_eq!(result, Ok(42));
    }

    #[test]
    fn test_execute_success_after_retry() {
        let strategy = FixedDelay::new(3, Duration::from_millis(1));
        let mut attempts = 0;
        let result: Result<i32, &str> = strategy.execute(|_| {
            attempts += 1;
            if attempts < 3 { Err("not yet") } else { Ok(42) }
        });
        assert_eq!(result, Ok(42));
        assert_eq!(attempts, 3);
    }

    #[test]
    fn test_execute_all_failures() {
        let strategy = FixedDelay::new(3, Duration::from_millis(1));
        let mut attempts = 0;
        let result: Result<i32, &str> = strategy.execute(|_| {
            attempts += 1;
            Err("always fails")
        });
        assert_eq!(result, Err("always fails"));
        assert_eq!(attempts, 3);
    }

    #[test]
    fn test_no_retry_execute() {
        let strategy = NoRetry::new();
        let mut attempts = 0;
        let result: Result<i32, &str> = strategy.execute(|_| {
            attempts += 1;
            Err("failed")
        });
        assert_eq!(result, Err("failed"));
        assert_eq!(attempts, 1);
    }
}
