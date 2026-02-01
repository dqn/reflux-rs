//! Judge data validation.

use crate::process::ReadMemory;
use crate::process::layout::judge;

/// Validate if the given address contains valid JudgeData.
///
/// Checks:
/// 1. State markers are in valid range (0-100)
/// 2. First 72 bytes (judgment values) are either all zeros or all valid
///
/// The judgment region contains 18 i32 values (P1/P2 judgments, combo breaks,
/// fast/slow counts, measure end markers). In song select state, these are
/// all zeros. During/after play, they contain valid counts.
pub fn validate_judge_data_candidate<R: ReadMemory + ?Sized>(reader: &R, addr: u64) -> bool {
    if !addr.is_multiple_of(4) {
        return false;
    }

    // Check state markers (must be 0-100)
    let marker1 = reader
        .read_i32(addr + judge::STATE_MARKER_1)
        .unwrap_or(-1);
    let marker2 = reader
        .read_i32(addr + judge::STATE_MARKER_2)
        .unwrap_or(-1);
    if !(0..=100).contains(&marker1) || !(0..=100).contains(&marker2) {
        return false;
    }

    // Read the judgment region (first 72 bytes = 18 i32 values)
    let Ok(bytes) = reader.read_bytes(addr, judge::INITIAL_ZERO_SIZE) else {
        return false;
    };

    // Check if all bytes are zero (song select state)
    let all_zeros = bytes.iter().all(|&b| b == 0);
    if all_zeros {
        return true;
    }

    // If not all zeros, verify each i32 value is in valid range
    // Each judgment value should be 0-3000 (MAX_NOTES), combo breaks 0-500, etc.
    for chunk in bytes.chunks_exact(4) {
        let value = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        // All values should be non-negative and reasonably bounded
        if !(0..=judge::MAX_NOTES).contains(&value) {
            return false;
        }
    }

    true
}
