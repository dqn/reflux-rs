//! Unlock data validation.

use crate::process::ReadMemory;

use super::super::constants::*;

/// Validate unlock_data address.
pub fn validate_unlock_data_address<R: ReadMemory + ?Sized>(reader: &R, addr: u64) -> bool {
    // First entry should have song_id around 1000, reasonable type and unlocks
    let song_id = match reader.read_i32(addr) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let unlock_type = match reader.read_i32(addr + 4) {
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
