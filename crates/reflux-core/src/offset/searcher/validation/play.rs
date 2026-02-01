//! Play settings and play data validation.

use crate::error::Result;
use crate::process::ReadMemory;
use crate::process::layout::settings;

use super::super::constants::*;

/// Validate if the given address contains valid PlaySettings.
///
/// Memory layout:
/// - 0x00: style (4 bytes, range 0-6)
/// - 0x04: gauge (4 bytes, range 0-4)
/// - 0x08: assist (4 bytes, range 0-5)
/// - 0x0C: flip (4 bytes, 0 or 1)
/// - 0x10: range (4 bytes, range 0-5)
pub fn validate_play_settings_at<R: ReadMemory + ?Sized>(reader: &R, addr: u64) -> Option<u64> {
    let style = reader.read_i32(addr).ok()?;
    let gauge = reader.read_i32(addr + 4).ok()?;
    let assist = reader.read_i32(addr + 8).ok()?;
    let flip = reader.read_i32(addr + 12).ok()?;
    let range = reader.read_i32(addr + 16).ok()?;

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
    let song_select_marker = reader
        .read_i32(addr.wrapping_sub(settings::SONG_SELECT_MARKER))
        .ok()?;
    if !(0..=1).contains(&song_select_marker) {
        return None;
    }

    Some(addr)
}

/// Validate if an address contains valid PlayData.
///
/// Initial state (all zeros) is NOT accepted during offset search.
/// We need actual play data with valid song_id to verify the offset is correct.
pub fn validate_play_data_address<R: ReadMemory + ?Sized>(reader: &R, addr: u64) -> Result<bool> {
    let song_id = reader.read_i32(addr).unwrap_or(-1);
    let difficulty = reader.read_i32(addr + 4).unwrap_or(-1);
    let ex_score = reader.read_i32(addr + 8).unwrap_or(-1);
    let miss_count = reader.read_i32(addr + 12).unwrap_or(-1);

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
