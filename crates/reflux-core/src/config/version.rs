use crate::error::Result;
use crate::process::ReadMemory;

/// Version prefix for INFINITAS
const VERSION_PREFIX: &str = "P2D:J:B:A:";

/// Expected version string length (e.g., "P2D:J:B:A:2024101500")
const VERSION_LENGTH: usize = 20;

/// Chunk size for memory reading (1MB)
const CHUNK_SIZE: usize = 1_000_000;

/// Maximum search size (80MB total)
const MAX_SEARCH_SIZE: usize = 80_000_000;

/// Expected offset for version string (approximately 4-5MB from base)
/// Historical analysis shows the current version is typically found early in memory.
const EXPECTED_VERSION_OFFSET: usize = 4_000_000;

/// Find the game version string from process memory
///
/// Searches for "P2D:J:B:A:YYYYMMDDNN" pattern using a two-phase approach:
/// 1. First, search in the expected region (around 4-5MB from base)
/// 2. If not found, fall back to full 80MB scan
///
/// Note: The first two occurrences are old 2016 builds, so we return the last found.
pub fn find_game_version<R: ReadMemory>(reader: &R, base_address: u64) -> Result<Option<String>> {
    // Phase 1: Quick search in expected region (4-8MB from base)
    // This covers most cases without a full 80MB scan
    let quick_search_start = base_address + EXPECTED_VERSION_OFFSET as u64;
    let quick_search_size = 4 * CHUNK_SIZE; // 4MB

    if let Some(version) = search_version_in_range(reader, quick_search_start, quick_search_size) {
        return Ok(Some(version));
    }

    // Phase 2: Full scan if quick search failed
    Ok(search_version_in_range(
        reader,
        base_address,
        MAX_SEARCH_SIZE,
    ))
}

/// Search for version string in a specific memory range
fn search_version_in_range<R: ReadMemory>(
    reader: &R,
    start_addr: u64,
    max_size: usize,
) -> Option<String> {
    let end_addr = start_addr + max_size as u64;
    let mut current_addr = start_addr;
    let mut last_found: Option<String> = None;
    let mut overlap_buffer = String::new();

    while current_addr < end_addr {
        let remaining = (end_addr - current_addr) as usize;
        let chunk_size = std::cmp::min(CHUNK_SIZE, remaining);

        let chunk = match reader.read_bytes(current_addr, chunk_size) {
            Ok(buf) => buf,
            Err(_) => break,
        };

        let text = decode_shift_jis(&chunk);
        let search_text = format!("{}{}", overlap_buffer, text);

        for i in 0..search_text.len().saturating_sub(VERSION_LENGTH) {
            if search_text[i..].starts_with(VERSION_PREFIX)
                && i + VERSION_LENGTH <= search_text.len()
            {
                let version = &search_text[i..i + VERSION_LENGTH];
                if is_valid_version(version) {
                    last_found = Some(version.to_string());
                }
            }
        }

        if text.len() >= VERSION_LENGTH {
            overlap_buffer = text[text.len() - VERSION_LENGTH + 1..].to_string();
        } else {
            overlap_buffer = text;
        }

        current_addr += chunk_size as u64;
    }

    last_found
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
