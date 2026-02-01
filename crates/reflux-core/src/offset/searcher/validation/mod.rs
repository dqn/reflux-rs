//! Validation functions for offset candidates.
//!
//! This module provides validation logic for verifying that candidate addresses
//! actually point to valid game data structures.

mod current_song;
mod data_map;
mod judge;
mod play;
mod song_list;
mod unlock;

use tracing::debug;

use crate::error::Result;
use crate::process::ReadMemory;
use crate::offset::OffsetsCollection;

use super::constants::*;

pub use current_song::validate_current_song_address;
pub use data_map::{validate_data_map_address, validate_data_map_node};
pub use judge::validate_judge_data_candidate;
pub use play::{validate_play_data_address, validate_play_settings_at};
pub use song_list::{count_songs_at_address, validate_new_version_text_table};
pub use unlock::validate_unlock_data_address;

/// Validation helper methods for OffsetSearcher.
pub trait OffsetValidation: ReadMemory {
    /// Validate if the given address contains valid JudgeData.
    fn validate_judge_data_candidate(&self, addr: u64) -> bool
    where
        Self: Sized,
    {
        validate_judge_data_candidate(self, addr)
    }

    /// Validate if the given address contains valid PlaySettings.
    fn validate_play_settings_at(&self, addr: u64) -> Option<u64>
    where
        Self: Sized,
    {
        validate_play_settings_at(self, addr)
    }

    /// Validate if an address contains valid PlayData.
    fn validate_play_data_address(&self, addr: u64) -> Result<bool>
    where
        Self: Sized,
    {
        validate_play_data_address(self, addr)
    }

    /// Validate if an address contains valid CurrentSong data.
    fn validate_current_song_address(&self, addr: u64) -> Result<bool>
    where
        Self: Sized,
    {
        validate_current_song_address(self, addr)
    }

    /// Validate data_map address.
    fn validate_data_map_address(&self, addr: u64) -> bool
    where
        Self: Sized,
    {
        validate_data_map_address(self, addr)
    }

    /// Validate unlock_data address.
    fn validate_unlock_data_address(&self, addr: u64) -> bool
    where
        Self: Sized,
    {
        validate_unlock_data_address(self, addr)
    }

    /// Validate a data map node.
    fn validate_data_map_node(&self, addr: u64) -> bool
    where
        Self: Sized,
    {
        validate_data_map_node(self, addr)
    }

    /// Count how many songs can be read from a given song list address.
    fn count_songs_at_address(&self, song_list_addr: u64) -> usize
    where
        Self: Sized,
    {
        count_songs_at_address(self, song_list_addr)
    }
}

// Implement the trait for any type that implements ReadMemory
impl<T: ReadMemory> OffsetValidation for T {}

/// Validate all offsets in a collection.
///
/// Performs structural validation of offset relationships and memory access checks.
pub fn validate_signature_offsets<R: ReadMemory>(reader: &R, offsets: &OffsetsCollection) -> bool {
    // Check required offsets are non-zero
    if offsets.song_list == 0 {
        debug!("Validation failed: song_list is zero");
        return false;
    }
    if offsets.judge_data == 0 {
        debug!("Validation failed: judge_data is zero");
        return false;
    }
    if offsets.play_settings == 0 {
        debug!("Validation failed: play_settings is zero");
        return false;
    }
    if offsets.play_data == 0 {
        debug!("Validation failed: play_data is zero");
        return false;
    }
    if offsets.current_song == 0 {
        debug!("Validation failed: current_song is zero");
        return false;
    }

    // Validate song list
    let song_count = count_songs_at_address(reader, offsets.song_list);
    let has_enough_songs = song_count >= MIN_EXPECTED_SONGS;
    let is_new_version =
        song_count >= 1 && validate_new_version_text_table(reader, offsets.song_list);

    if !has_enough_songs && !is_new_version {
        debug!(
            "Song list validation failed: count={}, new_version={}",
            song_count, is_new_version
        );
        return false;
    }
    debug!(
        "Song list validation passed: count={}, new_version={}",
        song_count, is_new_version
    );

    if !validate_judge_data_candidate(reader, offsets.judge_data) {
        debug!("Judge data validation failed at 0x{:X}", offsets.judge_data);
        return false;
    }
    debug!("Judge data validation passed at 0x{:X}", offsets.judge_data);

    if validate_play_settings_at(reader, offsets.play_settings).is_none() {
        debug!(
            "Play settings validation failed at 0x{:X}",
            offsets.play_settings
        );
        return false;
    }
    debug!(
        "Play settings validation passed at 0x{:X}",
        offsets.play_settings
    );

    if !validate_play_data_address(reader, offsets.play_data).unwrap_or(false) {
        debug!("Play data validation failed at 0x{:X}", offsets.play_data);
        return false;
    }
    debug!("Play data validation passed at 0x{:X}", offsets.play_data);

    if !validate_current_song_address(reader, offsets.current_song).unwrap_or(false) {
        debug!(
            "Current song validation failed at 0x{:X}",
            offsets.current_song
        );
        return false;
    }
    debug!(
        "Current song validation passed at 0x{:X}",
        offsets.current_song
    );

    // Validate data_map if present
    if offsets.data_map != 0 && !validate_data_map_address(reader, offsets.data_map) {
        debug!("Data map validation failed at 0x{:X}", offsets.data_map);
        return false;
    }
    if offsets.data_map != 0 {
        debug!("Data map validation passed at 0x{:X}", offsets.data_map);
    }

    // Validate unlock_data if present
    if offsets.unlock_data != 0 && !validate_unlock_data_address(reader, offsets.unlock_data) {
        debug!(
            "Unlock data validation failed at 0x{:X}",
            offsets.unlock_data
        );
        return false;
    }
    if offsets.unlock_data != 0 {
        debug!(
            "Unlock data validation passed at 0x{:X}",
            offsets.unlock_data
        );
    }

    // Validate relative distances between offsets
    let within_range = |actual: u64, expected: u64, range: u64| {
        if actual >= expected {
            actual - expected <= range
        } else {
            expected - actual <= range
        }
    };

    let judge_to_play = offsets.judge_data.wrapping_sub(offsets.play_settings);
    if !within_range(
        judge_to_play,
        JUDGE_TO_PLAY_SETTINGS,
        PLAY_SETTINGS_SEARCH_RANGE as u64,
    ) {
        debug!(
            "Relative distance validation failed: judge_data - play_settings = 0x{:X} (expected ~0x{:X})",
            judge_to_play, JUDGE_TO_PLAY_SETTINGS
        );
        return false;
    }

    let song_to_judge = offsets.song_list.wrapping_sub(offsets.judge_data);
    if !within_range(
        song_to_judge,
        JUDGE_TO_SONG_LIST,
        JUDGE_DATA_SEARCH_RANGE as u64,
    ) {
        debug!(
            "Relative distance validation failed: song_list - judge_data = 0x{:X} (expected ~0x{:X})",
            song_to_judge, JUDGE_TO_SONG_LIST
        );
        return false;
    }

    let play_data_delta = offsets.play_data.wrapping_sub(offsets.play_settings);
    if !within_range(
        play_data_delta,
        PLAY_SETTINGS_TO_PLAY_DATA,
        PLAY_DATA_SEARCH_RANGE as u64,
    ) {
        debug!(
            "Relative distance validation failed: play_data - play_settings = 0x{:X} (expected ~0x{:X})",
            play_data_delta, PLAY_SETTINGS_TO_PLAY_DATA
        );
        return false;
    }

    let current_song_delta = offsets.current_song.wrapping_sub(offsets.judge_data);
    if !within_range(
        current_song_delta,
        JUDGE_TO_CURRENT_SONG,
        CURRENT_SONG_SEARCH_RANGE as u64,
    ) {
        debug!(
            "Relative distance validation failed: current_song - judge_data = 0x{:X} (expected ~0x{:X})",
            current_song_delta, JUDGE_TO_CURRENT_SONG
        );
        return false;
    }

    debug!("All offset validations passed");
    true
}

/// Validate basic memory access for file-loaded offsets.
///
/// Skips relative distance checks which may differ between game versions.
pub fn validate_basic_memory_access<R: ReadMemory>(
    reader: &R,
    offsets: &OffsetsCollection,
) -> bool {
    // Check required offsets are non-zero
    if offsets.song_list == 0
        || offsets.judge_data == 0
        || offsets.play_settings == 0
        || offsets.play_data == 0
        || offsets.current_song == 0
    {
        debug!("Basic validation failed: some required offsets are zero");
        return false;
    }

    // Try to read from each offset to verify memory is accessible
    if reader.read_bytes(offsets.song_list, 64).is_err() {
        debug!(
            "Basic validation failed: cannot read song_list at 0x{:X}",
            offsets.song_list
        );
        return false;
    }

    if reader.read_bytes(offsets.judge_data, 32).is_err() {
        debug!(
            "Basic validation failed: cannot read judge_data at 0x{:X}",
            offsets.judge_data
        );
        return false;
    }

    if reader.read_bytes(offsets.play_settings, 32).is_err() {
        debug!(
            "Basic validation failed: cannot read play_settings at 0x{:X}",
            offsets.play_settings
        );
        return false;
    }

    if reader.read_bytes(offsets.play_data, 32).is_err() {
        debug!(
            "Basic validation failed: cannot read play_data at 0x{:X}",
            offsets.play_data
        );
        return false;
    }

    if reader.read_bytes(offsets.current_song, 16).is_err() {
        debug!(
            "Basic validation failed: cannot read current_song at 0x{:X}",
            offsets.current_song
        );
        return false;
    }

    debug!("Basic memory access validation passed for all offsets");
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::MockMemoryBuilder;
    use crate::process::layout::{judge, settings};

    #[test]
    fn test_validate_judge_data() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 50)
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        assert!(validate_judge_data_candidate(&reader, 0x1000));
    }

    #[test]
    fn test_validate_play_settings() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1) // song_select_marker
            .write_i32(marker_offset, 2) // style
            .write_i32(marker_offset + 4, 3) // gauge
            .write_i32(marker_offset + 8, 0) // assist
            .write_i32(marker_offset + 12, 0) // flip
            .write_i32(marker_offset + 16, 2) // range
            .build();

        assert!(validate_play_settings_at(&reader, 0x1000 + marker_offset as u64).is_some());
    }

    #[test]
    fn test_validate_play_data() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500) // song_id
            .write_i32(4, 3) // difficulty
            .write_i32(8, 2000) // ex_score
            .write_i32(12, 25) // miss_count
            .build();

        assert!(validate_play_data_address(&reader, 0x1000).unwrap());
    }

    #[test]
    fn test_validate_current_song() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 2500) // song_id
            .write_i32(4, 5) // difficulty
            .write_i32(8, 500) // field3
            .build();

        assert!(validate_current_song_address(&reader, 0x1000).unwrap());
    }
}
