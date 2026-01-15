use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Process not found: {0}")]
    ProcessNotFound(String),

    #[error("Failed to open process: {0}")]
    ProcessOpenFailed(String),

    #[error("Failed to read process memory at address {address:#x}: {message}")]
    MemoryReadFailed { address: u64, message: String },

    #[error("Invalid offset: {0}")]
    InvalidOffset(String),

    #[error("Offset version mismatch: expected {expected}, got {actual}")]
    OffsetVersionMismatch { expected: String, actual: String },

    #[error("Failed to search offset: {0}")]
    OffsetSearchFailed(String),

    #[error("Invalid game state")]
    InvalidGameState,

    #[error("Song database not loaded")]
    SongDatabaseNotLoaded,

    #[error("Config parse error: {0}")]
    ConfigParseError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Http(String),

    #[error("Encoding error: {0}")]
    EncodingError(String),

    #[error("API call failed: {endpoint} - {message}")]
    ApiError { endpoint: String, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Check if this error is a "file not found" error
    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound)
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        let message = if e.is_timeout() {
            format!("Request timed out: {}", e)
        } else if e.is_connect() {
            format!("Connection failed: {}", e)
        } else if e.is_request() {
            format!("Request error: {}", e)
        } else if let Some(status) = e.status() {
            format!("HTTP {} error: {}", status.as_u16(), e)
        } else {
            format!("HTTP error: {}", e)
        };
        Error::Http(message)
    }
}

/// Tracks API errors during a session for summary reporting
#[derive(Debug, Default)]
pub struct ApiErrorTracker {
    errors: std::sync::Mutex<Vec<ApiErrorRecord>>,
}

/// Record of a single API error
#[derive(Debug, Clone)]
pub struct ApiErrorRecord {
    pub endpoint: String,
    pub message: String,
    pub context: String,
}

impl ApiErrorTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an API error
    pub fn record(&self, endpoint: impl Into<String>, message: impl Into<String>, context: impl Into<String>) {
        if let Ok(mut errors) = self.errors.lock() {
            errors.push(ApiErrorRecord {
                endpoint: endpoint.into(),
                message: message.into(),
                context: context.into(),
            });
        }
    }

    /// Get the number of recorded errors
    pub fn count(&self) -> usize {
        self.errors.lock().map(|e| e.len()).unwrap_or(0)
    }

    /// Get a summary of errors grouped by endpoint
    pub fn summary(&self) -> Vec<(String, usize)> {
        let errors = match self.errors.lock() {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for error in errors.iter() {
            *counts.entry(error.endpoint.clone()).or_insert(0) += 1;
        }

        let mut result: Vec<_> = counts.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result
    }

    /// Clear all recorded errors
    pub fn clear(&self) {
        if let Ok(mut errors) = self.errors.lock() {
            errors.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_tracker_new() {
        let tracker = ApiErrorTracker::new();
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_api_error_tracker_record() {
        let tracker = ApiErrorTracker::new();
        tracker.record("endpoint1", "error message", "context");
        assert_eq!(tracker.count(), 1);
    }

    #[test]
    fn test_api_error_tracker_summary() {
        let tracker = ApiErrorTracker::new();
        tracker.record("endpoint1", "error1", "ctx1");
        tracker.record("endpoint1", "error2", "ctx2");
        tracker.record("endpoint2", "error3", "ctx3");

        let summary = tracker.summary();
        assert_eq!(summary.len(), 2);

        // endpoint1 should have 2 errors
        let endpoint1_count = summary.iter().find(|(e, _)| e == "endpoint1").map(|(_, c)| *c);
        assert_eq!(endpoint1_count, Some(2));

        // endpoint2 should have 1 error
        let endpoint2_count = summary.iter().find(|(e, _)| e == "endpoint2").map(|(_, c)| *c);
        assert_eq!(endpoint2_count, Some(1));
    }

    #[test]
    fn test_api_error_tracker_clear() {
        let tracker = ApiErrorTracker::new();
        tracker.record("endpoint", "error", "ctx");
        assert_eq!(tracker.count(), 1);

        tracker.clear();
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_error_is_not_found() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = Error::Io(io_err);
        assert!(err.is_not_found());

        let other_io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err2 = Error::Io(other_io_err);
        assert!(!err2.is_not_found());
    }
}
