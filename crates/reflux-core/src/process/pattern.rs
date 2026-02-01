//! Pattern matching utilities for memory searching.
//!
//! This module provides functions for searching byte patterns in memory buffers,
//! with support for wildcard bytes.

/// Find all occurrences of a pattern in a buffer.
///
/// Returns the byte offsets where the pattern starts.
///
/// # Arguments
///
/// * `buffer` - The buffer to search in
/// * `pattern` - The pattern to search for
///
/// # Example
///
/// ```
/// use reflux_core::process::pattern::find_pattern;
///
/// let buffer = [1, 2, 3, 1, 2, 3, 4];
/// let matches = find_pattern(&buffer, &[1, 2, 3]);
/// assert_eq!(matches, vec![0, 3]);
/// ```
pub fn find_pattern(buffer: &[u8], pattern: &[u8]) -> Vec<usize> {
    if pattern.is_empty() || pattern.len() > buffer.len() {
        return Vec::new();
    }

    buffer
        .windows(pattern.len())
        .enumerate()
        .filter_map(|(i, window)| if window == pattern { Some(i) } else { None })
        .collect()
}

/// Find all occurrences of a pattern with wildcards in a buffer.
///
/// Returns the byte offsets where the pattern starts.
///
/// # Arguments
///
/// * `buffer` - The buffer to search in
/// * `pattern` - The pattern to search for
/// * `wildcard_mask` - A mask indicating which bytes are wildcards (true = wildcard)
///
/// # Example
///
/// ```
/// use reflux_core::process::pattern::find_pattern_with_wildcards;
///
/// let buffer = [1, 2, 3, 1, 9, 3];
/// // Pattern [1, ??, 3] where ?? is a wildcard
/// let matches = find_pattern_with_wildcards(&buffer, &[1, 0, 3], &[false, true, false]);
/// assert_eq!(matches, vec![0, 3]);
/// ```
pub fn find_pattern_with_wildcards(
    buffer: &[u8],
    pattern: &[u8],
    wildcard_mask: &[bool],
) -> Vec<usize> {
    if pattern.is_empty() || pattern.len() > buffer.len() || pattern.len() != wildcard_mask.len() {
        return Vec::new();
    }

    let mut results = Vec::new();
    for i in 0..=(buffer.len() - pattern.len()) {
        let mut matches = true;
        for (j, (&byte, &is_wildcard)) in pattern.iter().zip(wildcard_mask.iter()).enumerate() {
            if !is_wildcard && buffer[i + j] != byte {
                matches = false;
                break;
            }
        }
        if matches {
            results.push(i);
        }
    }
    results
}

/// Find the first occurrence of a pattern in a buffer.
///
/// Returns the byte offset where the pattern starts, or None if not found.
pub fn find_first_pattern(buffer: &[u8], pattern: &[u8]) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > buffer.len() {
        return None;
    }

    buffer
        .windows(pattern.len())
        .position(|window| window == pattern)
}

/// Find the first occurrence of a pattern with wildcards in a buffer.
///
/// Returns the byte offset where the pattern starts, or None if not found.
pub fn find_first_pattern_with_wildcards(
    buffer: &[u8],
    pattern: &[u8],
    wildcard_mask: &[bool],
) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > buffer.len() || pattern.len() != wildcard_mask.len() {
        return None;
    }

    for i in 0..=(buffer.len() - pattern.len()) {
        let mut matches = true;
        for (j, (&byte, &is_wildcard)) in pattern.iter().zip(wildcard_mask.iter()).enumerate() {
            if !is_wildcard && buffer[i + j] != byte {
                matches = false;
                break;
            }
        }
        if matches {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_pattern_basic() {
        let buffer = [1, 2, 3, 4, 5, 1, 2, 3];
        let matches = find_pattern(&buffer, &[1, 2, 3]);
        assert_eq!(matches, vec![0, 5]);
    }

    #[test]
    fn test_find_pattern_no_match() {
        let buffer = [1, 2, 3, 4, 5];
        let matches = find_pattern(&buffer, &[6, 7, 8]);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_pattern_empty_pattern() {
        let buffer = [1, 2, 3];
        let matches = find_pattern(&buffer, &[]);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_pattern_pattern_larger_than_buffer() {
        let buffer = [1, 2];
        let matches = find_pattern(&buffer, &[1, 2, 3, 4, 5]);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_pattern_single_byte() {
        let buffer = [1, 2, 1, 3, 1];
        let matches = find_pattern(&buffer, &[1]);
        assert_eq!(matches, vec![0, 2, 4]);
    }

    #[test]
    fn test_find_pattern_with_wildcards_basic() {
        let buffer = [1, 2, 3, 1, 9, 3, 1, 5, 3];
        // Pattern: [1, ??, 3]
        let matches = find_pattern_with_wildcards(&buffer, &[1, 0, 3], &[false, true, false]);
        assert_eq!(matches, vec![0, 3, 6]);
    }

    #[test]
    fn test_find_pattern_with_wildcards_no_wildcards() {
        let buffer = [1, 2, 3, 4, 1, 2, 3];
        let matches = find_pattern_with_wildcards(&buffer, &[1, 2, 3], &[false, false, false]);
        assert_eq!(matches, vec![0, 4]);
    }

    #[test]
    fn test_find_pattern_with_wildcards_all_wildcards() {
        let buffer = [1, 2, 3, 4, 5];
        // All wildcards - should match at every valid position
        let matches = find_pattern_with_wildcards(&buffer, &[0, 0], &[true, true]);
        assert_eq!(matches, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_find_pattern_with_wildcards_mismatched_lengths() {
        let buffer = [1, 2, 3, 4, 5];
        let matches = find_pattern_with_wildcards(&buffer, &[1, 2, 3], &[false, false]);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_first_pattern() {
        let buffer = [1, 2, 3, 1, 2, 3];
        let result = find_first_pattern(&buffer, &[1, 2, 3]);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_find_first_pattern_not_found() {
        let buffer = [1, 2, 3, 4, 5];
        let result = find_first_pattern(&buffer, &[9, 9, 9]);
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_first_pattern_with_wildcards() {
        let buffer = [1, 9, 3, 1, 5, 3];
        let result =
            find_first_pattern_with_wildcards(&buffer, &[1, 0, 3], &[false, true, false]);
        assert_eq!(result, Some(0));
    }
}
