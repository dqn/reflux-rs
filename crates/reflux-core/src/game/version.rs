use crate::error::Result;
use crate::memory::MemoryReader;

/// Version prefix for INFINITAS
const VERSION_PREFIX: &str = "P2D:J:B:A:";

/// Expected version string length (e.g., "P2D:J:B:A:2024101500")
const VERSION_LENGTH: usize = 20;

/// Search buffer size (80MB)
const SEARCH_BUFFER_SIZE: usize = 80_000_000;

/// Find the game version string from process memory
///
/// Searches for "P2D:J:B:A:YYYYMMDDNN" pattern in the first 80MB of memory.
/// Note: The first two occurrences are old 2016 builds, so we return the last found.
pub fn find_game_version(reader: &MemoryReader, base_address: u64) -> Result<Option<String>> {
    // Read 80MB of memory
    let buffer = match reader.read_bytes(base_address, SEARCH_BUFFER_SIZE) {
        Ok(buf) => buf,
        Err(_) => return Ok(None),
    };

    // Convert to Shift-JIS string (lossy conversion for searching)
    let text = decode_shift_jis(&buffer);

    // Search for version prefix, keeping track of the last occurrence
    let mut last_found: Option<String> = None;

    for i in 0..text.len().saturating_sub(VERSION_LENGTH) {
        if text[i..].starts_with(VERSION_PREFIX) {
            if i + VERSION_LENGTH <= text.len() {
                let version = &text[i..i + VERSION_LENGTH];
                // Validate that the version looks correct (ends with digits)
                if is_valid_version(version) {
                    last_found = Some(version.to_string());
                }
            }
        }
    }

    Ok(last_found)
}

/// Check if the game version matches the offsets version
pub fn check_version_match(game_version: &str, offsets_version: &str) -> bool {
    game_version == offsets_version
}

/// Extract the date code from a version string (YYYYMMDDNN part)
pub fn extract_date_code(version: &str) -> Option<&str> {
    if version.starts_with(VERSION_PREFIX) && version.len() == VERSION_LENGTH {
        Some(&version[VERSION_PREFIX.len()..])
    } else {
        None
    }
}

/// Validate that a version string looks correct
fn is_valid_version(version: &str) -> bool {
    if !version.starts_with(VERSION_PREFIX) || version.len() != VERSION_LENGTH {
        return false;
    }

    // Check that the date code part is all digits
    let date_code = &version[VERSION_PREFIX.len()..];
    date_code.chars().all(|c| c.is_ascii_digit())
}

/// Decode Shift-JIS bytes to a string
/// Uses lossy conversion - invalid sequences are replaced with '?'
fn decode_shift_jis(bytes: &[u8]) -> String {
    // For version search, we only care about ASCII characters
    // So we can use a simple conversion that preserves ASCII
    bytes
        .iter()
        .map(|&b| {
            if b.is_ascii() {
                b as char
            } else {
                '\0' // Non-ASCII bytes are ignored for version search
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_version() {
        assert!(is_valid_version("P2D:J:B:A:2024101500"));
        assert!(is_valid_version("P2D:J:B:A:2023050100"));
        assert!(!is_valid_version("P2D:J:B:A:"));
        assert!(!is_valid_version("P2D:J:B:A:202410150")); // Too short
        assert!(!is_valid_version("P2D:J:B:A:20241015AB")); // Non-digits
        assert!(!is_valid_version("Invalid"));
    }

    #[test]
    fn test_extract_date_code() {
        assert_eq!(
            extract_date_code("P2D:J:B:A:2024101500"),
            Some("2024101500")
        );
        assert_eq!(extract_date_code("Invalid"), None);
    }

    #[test]
    fn test_check_version_match() {
        assert!(check_version_match(
            "P2D:J:B:A:2024101500",
            "P2D:J:B:A:2024101500"
        ));
        assert!(!check_version_match(
            "P2D:J:B:A:2024101500",
            "P2D:J:B:A:2024101501"
        ));
    }
}
