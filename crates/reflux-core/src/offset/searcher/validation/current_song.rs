//! Current song validation.

use crate::error::Result;
use crate::process::ReadMemory;

use super::super::utils::is_power_of_two;

/// Validate if an address contains valid CurrentSong data.
///
/// Initial state (all zeros) is NOT accepted during offset search.
/// We need actual song selection data to verify the offset is correct.
pub fn validate_current_song_address<R: ReadMemory + ?Sized>(reader: &R, addr: u64) -> Result<bool> {
    let song_id = reader.read_i32(addr).unwrap_or(-1);
    let difficulty = reader.read_i32(addr + 4).unwrap_or(-1);

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
    let field3 = reader.read_i32(addr + 8).unwrap_or(-1);
    if !(0..=10000).contains(&field3) {
        return Ok(false);
    }

    Ok(true)
}
