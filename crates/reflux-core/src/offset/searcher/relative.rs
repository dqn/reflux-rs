//! Relative offset search utilities
//!
//! Provides functions for finding offsets based on known relative positions
//! from anchor addresses.

use crate::error::{Error, Result};
use crate::process::ReadMemory;

use super::constants::*;
use super::validation::OffsetValidation;

/// Search near an expected address with validation
pub fn search_near_expected<R, F>(
    reader: &R,
    expected: u64,
    range: usize,
    validate: F,
) -> Option<u64>
where
    R: ReadMemory,
    F: Fn(&R, u64) -> bool,
{
    let range = range as u64;
    let step = 4u64;
    let mut delta = 0u64;

    while delta <= range {
        if delta == 0 {
            if expected.is_multiple_of(4) && validate(reader, expected) {
                return Some(expected);
            }
        } else {
            if expected >= delta {
                let addr = expected - delta;
                if addr.is_multiple_of(4) && validate(reader, addr) {
                    return Some(addr);
                }
            }

            let addr = expected + delta;
            if addr.is_multiple_of(4) && validate(reader, addr) {
                return Some(addr);
            }
        }

        delta += step;
    }

    None
}

/// Search for JudgeData near SongList using known offset relationship
pub fn search_judge_data_near_song_list<R: ReadMemory>(reader: &R, song_list: u64) -> Result<u64> {
    let expected = song_list.wrapping_sub(JUDGE_TO_SONG_LIST);
    search_near_expected(reader, expected, JUDGE_DATA_SEARCH_RANGE, |r, addr| {
        r.validate_judge_data_candidate(addr)
    })
    .ok_or_else(|| {
        Error::offset_search_failed_for(
            "judgeData",
            "No valid candidates found near SongList".to_string(),
        )
    })
}

/// Search for PlaySettings near JudgeData using known offset relationship
pub fn search_play_settings_near_judge_data<R: ReadMemory>(
    reader: &R,
    judge_data: u64,
) -> Result<u64> {
    let expected = judge_data.wrapping_sub(JUDGE_TO_PLAY_SETTINGS);
    search_near_expected(reader, expected, PLAY_SETTINGS_SEARCH_RANGE, |r, addr| {
        r.validate_play_settings_at(addr).is_some()
    })
    .ok_or_else(|| {
        Error::offset_search_failed_for(
            "playSettings",
            "No valid candidates found near JudgeData".to_string(),
        )
    })
}

/// Search for PlayData near PlaySettings using known offset relationship
pub fn search_play_data_near_play_settings<R: ReadMemory>(
    reader: &R,
    play_settings: u64,
) -> Result<u64> {
    let expected = play_settings.wrapping_add(PLAY_SETTINGS_TO_PLAY_DATA);
    search_near_expected(reader, expected, PLAY_DATA_SEARCH_RANGE, |r, addr| {
        r.validate_play_data_address(addr).unwrap_or(false)
    })
    .ok_or_else(|| {
        Error::offset_search_failed_for(
            "playData",
            "No valid candidates found near PlaySettings".to_string(),
        )
    })
}

/// Search for CurrentSong near JudgeData using known offset relationship
pub fn search_current_song_near_judge_data<R: ReadMemory>(
    reader: &R,
    judge_data: u64,
) -> Result<u64> {
    let expected = judge_data.wrapping_add(JUDGE_TO_CURRENT_SONG);
    search_near_expected(reader, expected, CURRENT_SONG_SEARCH_RANGE, |r, addr| {
        r.validate_current_song_address(addr).unwrap_or(false)
    })
    .ok_or_else(|| {
        Error::offset_search_failed_for(
            "currentSong",
            "No valid candidates found near JudgeData".to_string(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::MockMemoryBuilder;
    use crate::process::layout::judge;

    // ========================================================================
    // Version offset tests
    //
    // These tests verify that relative offsets between structures are stable
    // across game versions. The tolerance values used in searches should
    // accommodate any observed differences.
    // ========================================================================

    /// Known offsets from Version 1 (2025122400)
    mod version1 {
        pub const SONG_LIST: u64 = 0x14315A380;
        pub const JUDGE_DATA: u64 = 0x14280C00C;
        pub const PLAY_SETTINGS: u64 = 0x14255F124;
        pub const PLAY_DATA: u64 = 0x14255F3E4;
        pub const CURRENT_SONG: u64 = 0x14280C1F0;
    }

    /// Known offsets from Version 2 (2026012800)
    mod version2 {
        pub const SONG_LIST: u64 = 0x1431865A0;
        pub const JUDGE_DATA: u64 = 0x1428380EC;
        pub const PLAY_SETTINGS: u64 = 0x14258B144;
        pub const PLAY_DATA: u64 = 0x14258B3E4;
        pub const CURRENT_SONG: u64 = 0x1428382D0;
    }

    #[test]
    fn test_judge_to_song_list_offset_stability() {
        // Calculate actual offsets for both versions
        let v1_offset = version1::SONG_LIST - version1::JUDGE_DATA;
        let v2_offset = version2::SONG_LIST - version2::JUDGE_DATA;

        // Expected: ~0x94E3C8
        let expected = JUDGE_TO_SONG_LIST;

        // Both versions should be within search range
        let v1_diff = (v1_offset as i64 - expected as i64).unsigned_abs();
        let v2_diff = (v2_offset as i64 - expected as i64).unsigned_abs();

        assert!(
            v1_diff <= JUDGE_DATA_SEARCH_RANGE as u64,
            "Version 1 offset 0x{:X} exceeds tolerance from expected 0x{:X} (diff: 0x{:X}, range: 0x{:X})",
            v1_offset,
            expected,
            v1_diff,
            JUDGE_DATA_SEARCH_RANGE
        );
        assert!(
            v2_diff <= JUDGE_DATA_SEARCH_RANGE as u64,
            "Version 2 offset 0x{:X} exceeds tolerance from expected 0x{:X} (diff: 0x{:X}, range: 0x{:X})",
            v2_offset,
            expected,
            v2_diff,
            JUDGE_DATA_SEARCH_RANGE
        );
    }

    #[test]
    fn test_judge_to_play_settings_offset_stability() {
        // Calculate actual offsets for both versions
        let v1_offset = version1::JUDGE_DATA - version1::PLAY_SETTINGS;
        let v2_offset = version2::JUDGE_DATA - version2::PLAY_SETTINGS;

        // Expected: ~0x2ACEE8
        let expected = JUDGE_TO_PLAY_SETTINGS;

        // Both versions should be within search range
        let v1_diff = (v1_offset as i64 - expected as i64).unsigned_abs();
        let v2_diff = (v2_offset as i64 - expected as i64).unsigned_abs();

        assert!(
            v1_diff <= PLAY_SETTINGS_SEARCH_RANGE as u64,
            "Version 1 offset 0x{:X} exceeds tolerance from expected 0x{:X} (diff: 0x{:X}, range: 0x{:X})",
            v1_offset,
            expected,
            v1_diff,
            PLAY_SETTINGS_SEARCH_RANGE
        );
        assert!(
            v2_diff <= PLAY_SETTINGS_SEARCH_RANGE as u64,
            "Version 2 offset 0x{:X} exceeds tolerance from expected 0x{:X} (diff: 0x{:X}, range: 0x{:X})",
            v2_offset,
            expected,
            v2_diff,
            PLAY_SETTINGS_SEARCH_RANGE
        );
    }

    #[test]
    fn test_play_settings_to_play_data_offset_stability() {
        // Calculate actual offsets for both versions
        let v1_offset = version1::PLAY_DATA - version1::PLAY_SETTINGS;
        let v2_offset = version2::PLAY_DATA - version2::PLAY_SETTINGS;

        // Expected: ~0x2C0
        let expected = PLAY_SETTINGS_TO_PLAY_DATA;

        // Both versions should be within search range
        let v1_diff = (v1_offset as i64 - expected as i64).unsigned_abs();
        let v2_diff = (v2_offset as i64 - expected as i64).unsigned_abs();

        assert!(
            v1_diff <= PLAY_DATA_SEARCH_RANGE as u64,
            "Version 1 offset 0x{:X} exceeds tolerance from expected 0x{:X} (diff: 0x{:X}, range: 0x{:X})",
            v1_offset,
            expected,
            v1_diff,
            PLAY_DATA_SEARCH_RANGE
        );
        assert!(
            v2_diff <= PLAY_DATA_SEARCH_RANGE as u64,
            "Version 2 offset 0x{:X} exceeds tolerance from expected 0x{:X} (diff: 0x{:X}, range: 0x{:X})",
            v2_offset,
            expected,
            v2_diff,
            PLAY_DATA_SEARCH_RANGE
        );
    }

    #[test]
    fn test_judge_to_current_song_offset_stability() {
        // Calculate actual offsets for both versions
        let v1_offset = version1::CURRENT_SONG - version1::JUDGE_DATA;
        let v2_offset = version2::CURRENT_SONG - version2::JUDGE_DATA;

        // Expected: 0x1E4
        let expected = JUDGE_TO_CURRENT_SONG;

        // Both versions should be within search range
        let v1_diff = (v1_offset as i64 - expected as i64).unsigned_abs();
        let v2_diff = (v2_offset as i64 - expected as i64).unsigned_abs();

        assert!(
            v1_diff <= CURRENT_SONG_SEARCH_RANGE as u64,
            "Version 1 offset 0x{:X} exceeds tolerance from expected 0x{:X} (diff: 0x{:X}, range: 0x{:X})",
            v1_offset,
            expected,
            v1_diff,
            CURRENT_SONG_SEARCH_RANGE
        );
        assert!(
            v2_diff <= CURRENT_SONG_SEARCH_RANGE as u64,
            "Version 2 offset 0x{:X} exceeds tolerance from expected 0x{:X} (diff: 0x{:X}, range: 0x{:X})",
            v2_offset,
            expected,
            v2_diff,
            CURRENT_SONG_SEARCH_RANGE
        );

        // This offset should be exactly the same for both versions
        assert_eq!(
            v1_offset, v2_offset,
            "currentSong offset changed between versions (v1: 0x{:X}, v2: 0x{:X})",
            v1_offset, v2_offset
        );
    }

    // ========================================================================
    // Original tests
    // ========================================================================

    #[test]
    fn test_search_near_expected_exact_match() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 42) // Target value at base
            .build();

        let result = search_near_expected(&reader, 0x1000, 0x100, |r, addr| {
            r.read_i32(addr).unwrap_or(-1) == 42
        });

        assert_eq!(result, Some(0x1000));
    }

    #[test]
    fn test_search_near_expected_offset() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0x20, 42) // Target value at offset 0x20
            .build();

        // Search starting from 0x1000, should find at 0x1020
        let result = search_near_expected(&reader, 0x1000, 0x100, |r, addr| {
            r.read_i32(addr).unwrap_or(-1) == 42
        });

        assert_eq!(result, Some(0x1020));
    }

    #[test]
    fn test_search_near_expected_not_found() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .build();

        let result = search_near_expected(&reader, 0x1000, 0x50, |_, _| false);

        assert!(result.is_none());
    }

    #[test]
    fn test_search_judge_data_relative() {
        // Create a mock with valid JudgeData structure
        let song_list = 0x2000u64;
        let expected_judge = song_list.wrapping_sub(JUDGE_TO_SONG_LIST);

        let reader = MockMemoryBuilder::new()
            .base(expected_judge)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 50)
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        let result = search_judge_data_near_song_list(&reader, song_list);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_judge);
    }
}
