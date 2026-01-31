//! Validation functions for offset candidates
//!
//! This module provides validation logic for verifying that candidate addresses
//! actually point to valid game data structures.

use tracing::debug;

use crate::error::Result;
use crate::game::SongInfo;
use crate::memory::ReadMemory;
use crate::memory::layout::{judge, settings};
use crate::offset::OffsetsCollection;

use super::constants::*;
use super::utils::is_power_of_two;

/// Validation helper methods for OffsetSearcher
pub trait OffsetValidation: ReadMemory {
    /// Validate if the given address contains valid JudgeData
    ///
    /// Checks:
    /// 1. State markers are in valid range (0-100)
    /// 2. First 72 bytes (judgment values) are either all zeros or all valid
    ///
    /// The judgment region contains 18 i32 values (P1/P2 judgments, combo breaks,
    /// fast/slow counts, measure end markers). In song select state, these are
    /// all zeros. During/after play, they contain valid counts.
    fn validate_judge_data_candidate(&self, addr: u64) -> bool {
        if !addr.is_multiple_of(4) {
            return false;
        }

        // Check state markers (must be 0-100)
        let marker1 = self.read_i32(addr + judge::STATE_MARKER_1).unwrap_or(-1);
        let marker2 = self.read_i32(addr + judge::STATE_MARKER_2).unwrap_or(-1);
        if !(0..=100).contains(&marker1) || !(0..=100).contains(&marker2) {
            return false;
        }

        // Read the judgment region (first 72 bytes = 18 i32 values)
        let Ok(bytes) = self.read_bytes(addr, judge::INITIAL_ZERO_SIZE) else {
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

    /// Validate if the given address contains valid PlaySettings
    ///
    /// Memory layout:
    /// - 0x00: style (4 bytes, range 0-6)
    /// - 0x04: gauge (4 bytes, range 0-4)
    /// - 0x08: assist (4 bytes, range 0-5)
    /// - 0x0C: flip (4 bytes, 0 or 1)
    /// - 0x10: range (4 bytes, range 0-5)
    fn validate_play_settings_at(&self, addr: u64) -> Option<u64> {
        let style = self.read_i32(addr).ok()?;
        let gauge = self.read_i32(addr + 4).ok()?;
        let assist = self.read_i32(addr + 8).ok()?;
        let flip = self.read_i32(addr + 12).ok()?;
        let range = self.read_i32(addr + 16).ok()?;

        // Valid ranges check (aligned with C# implementation)
        if !(0..=6).contains(&style)
            || !(0..=4).contains(&gauge)
            || !(0..=5).contains(&assist)
            || !(0..=1).contains(&flip)
            || !(0..=5).contains(&range)
        {
            return None;
        }

        // Additional validation: song_select_marker should be 0 or 1
        let song_select_marker = self
            .read_i32(addr.wrapping_sub(settings::SONG_SELECT_MARKER))
            .ok()?;
        if !(0..=1).contains(&song_select_marker) {
            return None;
        }

        Some(addr)
    }

    /// Validate if an address contains valid PlayData
    ///
    /// Initial state (all zeros) is NOT accepted during offset search.
    /// We need actual play data with valid song_id to verify the offset is correct.
    fn validate_play_data_address(&self, addr: u64) -> Result<bool> {
        let song_id = self.read_i32(addr).unwrap_or(-1);
        let difficulty = self.read_i32(addr + 4).unwrap_or(-1);
        let ex_score = self.read_i32(addr + 8).unwrap_or(-1);
        let miss_count = self.read_i32(addr + 12).unwrap_or(-1);

        // Do NOT accept initial state (all zeros) during offset search.
        // Zero values can appear at wrong addresses - we need actual data to validate.
        // The game should have play data populated when we're searching for offsets.
        if song_id == 0 && difficulty == 0 && ex_score == 0 && miss_count == 0 {
            return Ok(false);
        }

        // Require song_id in valid IIDX range (>= 1000)
        let is_valid_play_data = (MIN_SONG_ID..=MAX_SONG_ID).contains(&song_id)
            && (0..=9).contains(&difficulty)
            && (0..=10000).contains(&ex_score)
            && (0..=3000).contains(&miss_count);

        Ok(is_valid_play_data)
    }

    /// Validate if an address contains valid CurrentSong data
    ///
    /// Initial state (all zeros) is NOT accepted during offset search.
    /// We need actual song selection data to verify the offset is correct.
    fn validate_current_song_address(&self, addr: u64) -> Result<bool> {
        let song_id = self.read_i32(addr).unwrap_or(-1);
        let difficulty = self.read_i32(addr + 4).unwrap_or(-1);

        // Do NOT accept initial state (zeros) during offset search.
        // Zero values can appear at wrong addresses - we need actual data to validate.
        // The game should have a song selected when we're searching for offsets.
        if song_id == 0 && difficulty == 0 {
            return Ok(false);
        }

        // song_id must be in realistic range (IIDX song IDs start from ~1000)
        if !(1000..=50000).contains(&song_id) {
            return Ok(false);
        }
        // Filter out powers of 2 which are likely memory artifacts
        if is_power_of_two(song_id as u32) {
            return Ok(false);
        }
        if !(0..=9).contains(&difficulty) {
            return Ok(false);
        }

        // Additional validation: check that the third field is reasonable
        let field3 = self.read_i32(addr + 8).unwrap_or(-1);
        if !(0..=10000).contains(&field3) {
            return Ok(false);
        }

        Ok(true)
    }

    /// Validate data_map address
    fn validate_data_map_address(&self, addr: u64) -> bool {
        // DataMap structure: table_start at addr, table_end at addr+8
        let table_start = match self.read_u64(addr) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let table_end = match self.read_u64(addr + 8) {
            Ok(v) => v,
            Err(_) => return false,
        };

        if table_end <= table_start {
            return false;
        }

        let size = table_end - table_start;
        // Valid size range: 8KB to 16MB
        (0x2000..=0x1000000).contains(&size)
    }

    /// Validate unlock_data address
    fn validate_unlock_data_address(&self, addr: u64) -> bool {
        // First entry should have song_id around 1000, reasonable type and unlocks
        let song_id = match self.read_i32(addr) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let unlock_type = match self.read_i32(addr + 4) {
            Ok(v) => v,
            Err(_) => return false,
        };

        // song_id should be in valid range
        if !(MIN_SONG_ID..=MAX_SONG_ID).contains(&song_id) {
            return false;
        }

        // unlock_type should be 0-3
        if !(0..=3).contains(&unlock_type) {
            return false;
        }

        true
    }

    /// Validate a data map node
    fn validate_data_map_node(&self, addr: u64) -> bool {
        let buffer = match self.read_bytes(addr, 64) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        if buffer.len() < 52 {
            return false;
        }

        let diff = i32::from_le_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
        let song_id = i32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
        let playtype = i32::from_le_bytes([buffer[24], buffer[25], buffer[26], buffer[27]]);
        let score = u32::from_le_bytes([buffer[32], buffer[33], buffer[34], buffer[35]]);
        let miss_count = u32::from_le_bytes([buffer[36], buffer[37], buffer[38], buffer[39]]);
        let lamp = i32::from_le_bytes([buffer[48], buffer[49], buffer[50], buffer[51]]);

        if !(0..=4).contains(&diff) {
            return false;
        }
        if !(0..=1).contains(&playtype) {
            return false;
        }
        if !(MIN_SONG_ID..=MAX_SONG_ID).contains(&song_id) {
            return false;
        }
        if score > 200_000 {
            return false;
        }
        if miss_count > 10_000 && miss_count != u32::MAX {
            return false;
        }
        if !(0..=7).contains(&lamp) {
            return false;
        }

        true
    }

    /// Count how many songs can be read from a given song list address
    ///
    /// This function counts songs until:
    /// - MIN_EXPECTED_SONGS (1000) is reached (early termination for performance)
    /// - MAX_SONGS_TO_CHECK (5000) is reached
    /// - Too many consecutive failures occur
    fn count_songs_at_address(&self, song_list_addr: u64) -> usize
    where
        Self: Sized,
    {
        let mut count = 0;
        let mut consecutive_failures = 0;
        let mut current_position: u64 = 0;

        const MAX_SONGS_TO_CHECK: usize = 5000;
        const MAX_CONSECUTIVE_FAILURES: u32 = 10;

        while count < MAX_SONGS_TO_CHECK {
            // Early termination: once we have enough songs, no need to count more
            if count >= MIN_EXPECTED_SONGS {
                debug!(
                    "    Reached {} songs, stopping early (enough for validation)",
                    count
                );
                return count;
            }
            let address = song_list_addr + current_position;

            match SongInfo::read_from_memory(self, address) {
                Ok(Some(song)) if !song.title.is_empty() => {
                    if count < 3
                        && let Ok(full_buffer) = self.read_bytes(address, SongInfo::MEMORY_SIZE)
                    {
                        let id_offset = 256 + 368; // SONG_ID_OFFSET
                        debug!(
                            "    Song {}: id={}, title={:?} at 0x{:X}",
                            count, song.id, song.title, address
                        );
                        debug!("      First 32 bytes: {:02X?}", &full_buffer[0..32]);
                        debug!(
                            "      Bytes at id_offset ({}): {:02X?}",
                            id_offset,
                            &full_buffer[id_offset..id_offset + 8]
                        );
                    }
                    count += 1;
                    consecutive_failures = 0;
                }
                Ok(Some(song)) => {
                    debug!("    Song at 0x{:X}: empty title (id={})", address, song.id);
                    consecutive_failures += 1;
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        debug!(
                            "    Stopping after {} consecutive empty/invalid entries",
                            consecutive_failures
                        );
                        break;
                    }
                }
                Ok(None) => {
                    if count < 5
                        && let Ok(bytes) = self.read_bytes(address, 16)
                    {
                        debug!(
                            "    Song at 0x{:X}: first 4 bytes zero, raw: {:02X?}",
                            address, bytes
                        );
                    }
                    consecutive_failures += 1;
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        debug!(
                            "    Stopping after {} consecutive empty/invalid entries",
                            consecutive_failures
                        );
                        break;
                    }
                }
                Err(e) => {
                    debug!("    Song at 0x{:X}: read error: {}", address, e);
                    break;
                }
            }

            current_position += SongInfo::MEMORY_SIZE as u64;
        }

        count
    }
}

// Implement the trait for any type that implements ReadMemory
impl<T: ReadMemory> OffsetValidation for T {}

/// Validate all offsets in a collection
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
    let song_count = reader.count_songs_at_address(offsets.song_list);
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

    if !reader.validate_judge_data_candidate(offsets.judge_data) {
        debug!("Judge data validation failed at 0x{:X}", offsets.judge_data);
        return false;
    }
    debug!("Judge data validation passed at 0x{:X}", offsets.judge_data);

    if reader
        .validate_play_settings_at(offsets.play_settings)
        .is_none()
    {
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

    if !reader
        .validate_play_data_address(offsets.play_data)
        .unwrap_or(false)
    {
        debug!("Play data validation failed at 0x{:X}", offsets.play_data);
        return false;
    }
    debug!("Play data validation passed at 0x{:X}", offsets.play_data);

    if !reader
        .validate_current_song_address(offsets.current_song)
        .unwrap_or(false)
    {
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
    if offsets.data_map != 0 && !reader.validate_data_map_address(offsets.data_map) {
        debug!("Data map validation failed at 0x{:X}", offsets.data_map);
        return false;
    }
    if offsets.data_map != 0 {
        debug!("Data map validation passed at 0x{:X}", offsets.data_map);
    }

    // Validate unlock_data if present
    if offsets.unlock_data != 0 && !reader.validate_unlock_data_address(offsets.unlock_data) {
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

/// Validate basic memory access for file-loaded offsets
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

/// Validate if address is a valid text table for new INFINITAS version
pub fn validate_new_version_text_table<R: ReadMemory>(reader: &R, text_base: u64) -> bool {
    // Check metadata table at text_base + 0x7E0
    let metadata_addr = text_base + SongInfo::METADATA_TABLE_OFFSET as u64;

    // Read first metadata entry
    let Ok(metadata) = reader.read_bytes(metadata_addr, 8) else {
        return false;
    };

    let song_id = i32::from_le_bytes([metadata[0], metadata[1], metadata[2], metadata[3]]);
    let folder = i32::from_le_bytes([metadata[4], metadata[5], metadata[6], metadata[7]]);

    // Validate: first song in list should be song_id ~1000-2000 range
    let valid_song_id = (1000..=5000).contains(&song_id);
    let valid_folder = (1..=50).contains(&folder);

    if valid_song_id && valid_folder {
        debug!(
            "  New version text table validation passed: song_id={}, folder={} at metadata 0x{:X}",
            song_id, folder, metadata_addr
        );
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MockMemoryBuilder;

    #[test]
    fn test_validate_judge_data() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 50)
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        assert!(reader.validate_judge_data_candidate(0x1000));
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

        assert!(
            reader
                .validate_play_settings_at(0x1000 + marker_offset as u64)
                .is_some()
        );
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

        assert!(reader.validate_play_data_address(0x1000).unwrap());
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

        assert!(reader.validate_current_song_address(0x1000).unwrap());
    }
}
