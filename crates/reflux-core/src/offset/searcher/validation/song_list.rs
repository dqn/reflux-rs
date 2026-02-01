//! Song list validation.

use tracing::debug;

use crate::chart::SongInfo;
use crate::process::ReadMemory;

use super::super::constants::MIN_EXPECTED_SONGS;

/// Count how many songs can be read from a given song list address.
///
/// This function counts songs until:
/// - MIN_EXPECTED_SONGS (1000) is reached (early termination for performance)
/// - MAX_SONGS_TO_CHECK (5000) is reached
/// - Too many consecutive failures occur
pub fn count_songs_at_address<R: ReadMemory>(reader: &R, song_list_addr: u64) -> usize {
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

        match SongInfo::read_from_memory(reader, address) {
            Ok(Some(song)) if !song.title.is_empty() => {
                if count < 3
                    && let Ok(full_buffer) = reader.read_bytes(address, SongInfo::MEMORY_SIZE)
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
                    && let Ok(bytes) = reader.read_bytes(address, 16)
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

/// Validate if address is a valid text table for new INFINITAS version.
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
