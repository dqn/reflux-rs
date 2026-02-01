//! Data map validation.

use crate::process::ReadMemory;

use super::super::constants::*;

/// Validate data_map address.
pub fn validate_data_map_address<R: ReadMemory + ?Sized>(reader: &R, addr: u64) -> bool {
    // DataMap structure: table_start at addr, table_end at addr+8
    let table_start = match reader.read_u64(addr) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let table_end = match reader.read_u64(addr + 8) {
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

/// Validate a data map node.
pub fn validate_data_map_node<R: ReadMemory + ?Sized>(reader: &R, addr: u64) -> bool {
    let buffer = match reader.read_bytes(addr, 64) {
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
