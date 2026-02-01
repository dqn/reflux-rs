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

impl<'a, R: ReadMemory> OffsetSearcher<'a, R> {
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
                .unwrap_or(false)
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
                this.reader
                    .validate_play_data_address(inferred_play_data)
                    .unwrap_or(false)
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
            this.reader
                .validate_play_data_address(addr)
                .unwrap_or(false)
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
            this.reader
                .validate_current_song_address(addr)
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for currentSong near JudgeData".to_string(),
            )
        })
    }
}
