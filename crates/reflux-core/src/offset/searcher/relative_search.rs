//! Relative offset search functionality
//!
//! This module implements the relative offset search strategy, which finds
//! game data structures by searching near expected positions calculated from
//! known anchor points.

use crate::error::{Error, Result};
use crate::process::ReadMemory;

use super::OffsetSearcher;
use super::constants::*;
use super::validation::OffsetValidation;

impl<R: ReadMemory> OffsetSearcher<'_, R> {
    /// Search for an address near an expected location with validation
    pub(crate) fn search_near_expected<F>(
        &self,
        expected: u64,
        range: usize,
        validate: F,
    ) -> Option<u64>
    where
        F: Fn(&Self, u64) -> bool,
    {
        let range = range as u64;
        let step = 4u64;
        let mut delta = 0u64;

        while delta <= range {
            if delta == 0 {
                if expected.is_multiple_of(4) && validate(self, expected) {
                    return Some(expected);
                }
            } else {
                if expected >= delta {
                    let addr = expected - delta;
                    if addr.is_multiple_of(4) && validate(self, addr) {
                        return Some(addr);
                    }
                }

                let addr = expected + delta;
                if addr.is_multiple_of(4) && validate(self, addr) {
                    return Some(addr);
                }
            }

            delta += step;
        }

        None
    }

    /// Search for JudgeData near SongList using relative offset
    pub(crate) fn search_judge_data_near_song_list(&self, song_list: u64) -> Result<u64> {
        let expected = song_list.wrapping_sub(JUDGE_TO_SONG_LIST);

        // First, try to find a candidate where both JudgeData and the inferred
        // CurrentSong position are valid. This cross-validation is more reliable.
        let result = self.search_near_expected(expected, JUDGE_DATA_SEARCH_RANGE, |this, addr| {
            if !this.reader.validate_judge_data_candidate(addr) {
                return false;
            }
            // Cross-validate: check if CurrentSong at expected relative position is valid
            let inferred_current_song = addr.wrapping_add(JUDGE_TO_CURRENT_SONG);
            this.reader
                .validate_current_song_address(inferred_current_song)
        });

        if let Some(addr) = result {
            return Ok(addr);
        }

        // Fallback: just validate JudgeData structure itself
        self.search_near_expected(expected, JUDGE_DATA_SEARCH_RANGE, |this, addr| {
            this.reader.validate_judge_data_candidate(addr)
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for judgeData near SongList".to_string(),
            )
        })
    }

    /// Search for PlaySettings near JudgeData using relative offset
    pub(crate) fn search_play_settings_near_judge_data(&self, judge_data: u64) -> Result<u64> {
        let expected = judge_data.wrapping_sub(JUDGE_TO_PLAY_SETTINGS);

        // First, try to find a candidate where both PlaySettings and the inferred
        // PlayData position are valid. This cross-validation is more reliable.
        let result =
            self.search_near_expected(expected, PLAY_SETTINGS_SEARCH_RANGE, |this, addr| {
                if this.reader.validate_play_settings_at(addr).is_none() {
                    return false;
                }
                // Cross-validate: check if PlayData at expected relative position is valid
                let inferred_play_data = addr.wrapping_add(PLAY_SETTINGS_TO_PLAY_DATA);
                this.reader.validate_play_data_address(inferred_play_data)
            });

        if let Some(addr) = result {
            return Ok(addr);
        }

        // Fallback: just validate PlaySettings structure itself
        self.search_near_expected(expected, PLAY_SETTINGS_SEARCH_RANGE, |this, addr| {
            this.reader.validate_play_settings_at(addr).is_some()
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for playSettings near JudgeData".to_string(),
            )
        })
    }

    /// Search for PlayData near PlaySettings using relative offset
    pub(crate) fn search_play_data_near_play_settings(&self, play_settings: u64) -> Result<u64> {
        let expected = play_settings.wrapping_add(PLAY_SETTINGS_TO_PLAY_DATA);
        self.search_near_expected(expected, PLAY_DATA_SEARCH_RANGE, |this, addr| {
            this.reader.validate_play_data_address(addr)
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for playData near PlaySettings".to_string(),
            )
        })
    }

    /// Search for CurrentSong near JudgeData using relative offset
    pub(crate) fn search_current_song_near_judge_data(&self, judge_data: u64) -> Result<u64> {
        let expected = judge_data.wrapping_add(JUDGE_TO_CURRENT_SONG);
        self.search_near_expected(expected, CURRENT_SONG_SEARCH_RANGE, |this, addr| {
            this.reader.validate_current_song_address(addr)
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for currentSong near JudgeData".to_string(),
            )
        })
    }
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
        let v1_offset = version1::SONG_LIST - version1::JUDGE_DATA;
        let v2_offset = version2::SONG_LIST - version2::JUDGE_DATA;
        let expected = JUDGE_TO_SONG_LIST;

        let v1_diff = (v1_offset as i64 - expected as i64).unsigned_abs();
        let v2_diff = (v2_offset as i64 - expected as i64).unsigned_abs();

        assert!(
            v1_diff <= JUDGE_DATA_SEARCH_RANGE as u64,
            "Version 1 offset 0x{:X} exceeds tolerance from expected 0x{:X}",
            v1_offset,
            expected
        );
        assert!(
            v2_diff <= JUDGE_DATA_SEARCH_RANGE as u64,
            "Version 2 offset 0x{:X} exceeds tolerance from expected 0x{:X}",
            v2_offset,
            expected
        );
    }

    #[test]
    fn test_judge_to_play_settings_offset_stability() {
        let v1_offset = version1::JUDGE_DATA - version1::PLAY_SETTINGS;
        let v2_offset = version2::JUDGE_DATA - version2::PLAY_SETTINGS;
        let expected = JUDGE_TO_PLAY_SETTINGS;

        let v1_diff = (v1_offset as i64 - expected as i64).unsigned_abs();
        let v2_diff = (v2_offset as i64 - expected as i64).unsigned_abs();

        assert!(
            v1_diff <= PLAY_SETTINGS_SEARCH_RANGE as u64,
            "Version 1 offset 0x{:X} exceeds tolerance from expected 0x{:X}",
            v1_offset,
            expected
        );
        assert!(
            v2_diff <= PLAY_SETTINGS_SEARCH_RANGE as u64,
            "Version 2 offset 0x{:X} exceeds tolerance from expected 0x{:X}",
            v2_offset,
            expected
        );
    }

    #[test]
    fn test_play_settings_to_play_data_offset_stability() {
        let v1_offset = version1::PLAY_DATA - version1::PLAY_SETTINGS;
        let v2_offset = version2::PLAY_DATA - version2::PLAY_SETTINGS;
        let expected = PLAY_SETTINGS_TO_PLAY_DATA;

        let v1_diff = (v1_offset as i64 - expected as i64).unsigned_abs();
        let v2_diff = (v2_offset as i64 - expected as i64).unsigned_abs();

        assert!(
            v1_diff <= PLAY_DATA_SEARCH_RANGE as u64,
            "Version 1 offset 0x{:X} exceeds tolerance from expected 0x{:X}",
            v1_offset,
            expected
        );
        assert!(
            v2_diff <= PLAY_DATA_SEARCH_RANGE as u64,
            "Version 2 offset 0x{:X} exceeds tolerance from expected 0x{:X}",
            v2_offset,
            expected
        );
    }

    #[test]
    fn test_judge_to_current_song_offset_stability() {
        let v1_offset = version1::CURRENT_SONG - version1::JUDGE_DATA;
        let v2_offset = version2::CURRENT_SONG - version2::JUDGE_DATA;
        let expected = JUDGE_TO_CURRENT_SONG;

        let v1_diff = (v1_offset as i64 - expected as i64).unsigned_abs();
        let v2_diff = (v2_offset as i64 - expected as i64).unsigned_abs();

        assert!(
            v1_diff <= CURRENT_SONG_SEARCH_RANGE as u64,
            "Version 1 offset 0x{:X} exceeds tolerance from expected 0x{:X}",
            v1_offset,
            expected
        );
        assert!(
            v2_diff <= CURRENT_SONG_SEARCH_RANGE as u64,
            "Version 2 offset 0x{:X} exceeds tolerance from expected 0x{:X}",
            v2_offset,
            expected
        );
        assert_eq!(
            v1_offset, v2_offset,
            "currentSong offset changed between versions"
        );
    }

    // ========================================================================
    // Search function tests
    // ========================================================================

    #[test]
    fn test_search_near_expected_exact_match() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 42)
            .build();
        let searcher = OffsetSearcher::new(&reader);

        let result = searcher.search_near_expected(0x1000, 0x100, |this, addr| {
            this.reader.read_i32(addr).unwrap_or(-1) == 42
        });

        assert_eq!(result, Some(0x1000));
    }

    #[test]
    fn test_search_near_expected_offset() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0x20, 42)
            .build();
        let searcher = OffsetSearcher::new(&reader);

        let result = searcher.search_near_expected(0x1000, 0x100, |this, addr| {
            this.reader.read_i32(addr).unwrap_or(-1) == 42
        });

        assert_eq!(result, Some(0x1020));
    }

    #[test]
    fn test_search_near_expected_not_found() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .build();
        let searcher = OffsetSearcher::new(&reader);

        let result = searcher.search_near_expected(0x1000, 0x50, |_, _| false);

        assert!(result.is_none());
    }

    #[test]
    fn test_search_judge_data_relative() {
        let song_list = 0x2000u64;
        let expected_judge = song_list.wrapping_sub(JUDGE_TO_SONG_LIST);

        let reader = MockMemoryBuilder::new()
            .base(expected_judge)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 50)
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();
        let searcher = OffsetSearcher::new(&reader);

        let result = searcher.search_judge_data_near_song_list(song_list);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_judge);
    }
}
