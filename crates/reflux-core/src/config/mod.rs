//! Configuration and support files.
//!
//! This module contains types for configuration and support files:
//! - `CustomTypes` - custom unlock type overrides
//! - `EncodingFixes` - encoding fixes for song titles and artists
//! - Version detection utilities
//! - Polling, retry, and database configuration constants

mod custom_types;
mod encoding_fixes;
mod version;

pub use custom_types::*;
pub use encoding_fixes::*;
pub use version::*;

/// Memory read retry configuration.
///
/// Exponential backoff: 100ms → 200ms → 400ms → 800ms → 1600ms = total ~3.1s max.
/// Longer delays reduce false disconnection detection from transient failures.
pub mod retry {
    /// Maximum number of retry attempts for memory read operations.
    pub const MAX_READ_RETRIES: u32 = 5;

    /// Delay (in ms) for each retry attempt (exponential backoff).
    pub const RETRY_DELAYS_MS: [u64; 5] = [100, 200, 400, 800, 1600];
}

/// Result screen polling configuration.
///
/// Exponential backoff: 50+50+100+100+200+200+300+300+500+500 = 2.3 seconds max.
/// Faster initial polling catches quick data availability, while exponential
/// backoff reduces CPU usage if data takes longer to populate.
pub mod polling {
    /// Delay (in ms) for each polling attempt on result screen.
    pub const POLL_DELAYS_MS: [u64; 10] = [50, 50, 100, 100, 200, 200, 300, 300, 500, 500];
}

/// Song database loading configuration.
pub mod database {
    use std::time::Duration;

    /// Maximum number of attempts to load the song database.
    pub const MAX_LOAD_ATTEMPTS: u32 = 12;

    /// Delay between retry attempts.
    pub const RETRY_DELAY: Duration = Duration::from_secs(5);

    /// Extra delay for data initialization.
    pub const EXTRA_DELAY: Duration = Duration::from_secs(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_constants() {
        assert_eq!(retry::MAX_READ_RETRIES, 5);
        assert_eq!(retry::RETRY_DELAYS_MS.len(), 5);
    }

    #[test]
    fn test_polling_constants() {
        assert_eq!(polling::POLL_DELAYS_MS.len(), 10);
        let total: u64 = polling::POLL_DELAYS_MS.iter().sum();
        assert_eq!(total, 2300); // 2.3 seconds
    }

    #[test]
    fn test_database_constants() {
        assert_eq!(database::MAX_LOAD_ATTEMPTS, 12);
        assert_eq!(database::RETRY_DELAY.as_secs(), 5);
        assert_eq!(database::EXTRA_DELAY.as_secs(), 1);
    }
}
