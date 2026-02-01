//! Status information for debugging

use serde::Serialize;

use crate::chart::SongInfo;
use crate::process::ReadMemory;
use crate::offset::{OffsetSearcher, OffsetsCollection};

/// Validation result for an individual offset
#[derive(Debug, Clone, Serialize)]
pub struct OffsetValidation {
    pub name: String,
    pub address: u64,
    pub valid: bool,
    pub reason: String,
}

/// Status of all offsets
#[derive(Debug, Clone, Serialize)]
pub struct OffsetStatus {
    pub song_list: OffsetValidation,
    pub judge_data: OffsetValidation,
    pub play_settings: OffsetValidation,
    pub play_data: OffsetValidation,
    pub current_song: OffsetValidation,
    pub data_map: OffsetValidation,
    pub unlock_data: OffsetValidation,
}

/// Complete status information
#[derive(Debug, Clone, Serialize)]
pub struct StatusInfo {
    /// Game process PID
    pub pid: u32,
    /// Game base address
    pub base_address: u64,
    /// Game module size
    pub module_size: u64,
    /// Game version string
    pub version: Option<String>,
    /// Offset status
    pub offsets: OffsetStatus,
    /// Number of songs found in memory
    pub song_count: usize,
    /// Currently selected song (if available)
    pub current_song: Option<CurrentSongInfo>,
    /// Overall validation status
    pub all_valid: bool,
}

/// Information about the currently selected song
#[derive(Debug, Clone, Serialize)]
pub struct CurrentSongInfo {
    pub song_id: u32,
    pub difficulty: u8,
    pub title: Option<String>,
}

impl StatusInfo {
    /// Collect status information from the game process
    pub fn collect<R: ReadMemory>(
        reader: &R,
        pid: u32,
        base_address: u64,
        module_size: u64,
        version: Option<String>,
        offsets: &OffsetsCollection,
    ) -> Self {
        let searcher = OffsetSearcher::new(reader);

        // Validate each offset
        let song_list = validate_song_list(reader, offsets.song_list);
        let judge_data = validate_judge_data(reader, offsets.judge_data);
        let play_settings = validate_play_settings(reader, offsets.play_settings);
        let play_data = validate_play_data(reader, offsets.play_data);
        let current_song_offset = validate_current_song(reader, offsets.current_song);
        let data_map = validate_data_map(reader, offsets.data_map);
        let unlock_data = validate_unlock_data(reader, offsets.unlock_data);

        let offsets_status = OffsetStatus {
            song_list,
            judge_data,
            play_settings,
            play_data,
            current_song: current_song_offset,
            data_map,
            unlock_data,
        };

        // Count songs
        let song_count = count_songs_at_address(reader, offsets.song_list);

        // Get current song info
        let current_song = get_current_song_info(reader, offsets.current_song, offsets.song_list);

        // Overall validation
        let all_valid = searcher.validate_signature_offsets(offsets);

        StatusInfo {
            pid,
            base_address,
            module_size,
            version,
            offsets: offsets_status,
            song_count,
            current_song,
            all_valid,
        }
    }
}

fn validate_song_list<R: ReadMemory>(reader: &R, addr: u64) -> OffsetValidation {
    if addr == 0 {
        return OffsetValidation {
            name: "songList".to_string(),
            address: addr,
            valid: false,
            reason: "Address is zero".to_string(),
        };
    }

    // Check if we can read from the address
    match reader.read_bytes(addr, 64) {
        Ok(bytes) => {
            // Check if first entry looks like song data (title should start with printable chars)
            let has_title = bytes.iter().take(32).any(|&b| (0x20..0x80).contains(&b));
            if has_title {
                OffsetValidation {
                    name: "songList".to_string(),
                    address: addr,
                    valid: true,
                    reason: "First entry has readable title data".to_string(),
                }
            } else {
                // Check metadata table for new version
                let metadata_addr = addr + SongInfo::METADATA_TABLE_OFFSET as u64;
                match reader.read_bytes(metadata_addr, 8) {
                    Ok(meta) => {
                        let song_id = i32::from_le_bytes([meta[0], meta[1], meta[2], meta[3]]);
                        let folder = i32::from_le_bytes([meta[4], meta[5], meta[6], meta[7]]);
                        if (1000..=50000).contains(&song_id) && (1..=50).contains(&folder) {
                            OffsetValidation {
                                name: "songList".to_string(),
                                address: addr,
                                valid: true,
                                reason: format!(
                                    "Metadata table valid: song_id={}, folder={}",
                                    song_id, folder
                                ),
                            }
                        } else {
                            OffsetValidation {
                                name: "songList".to_string(),
                                address: addr,
                                valid: false,
                                reason: format!(
                                    "No valid title, metadata invalid: song_id={}, folder={}",
                                    song_id, folder
                                ),
                            }
                        }
                    }
                    Err(e) => OffsetValidation {
                        name: "songList".to_string(),
                        address: addr,
                        valid: false,
                        reason: format!("Failed to read metadata table: {}", e),
                    },
                }
            }
        }
        Err(e) => OffsetValidation {
            name: "songList".to_string(),
            address: addr,
            valid: false,
            reason: format!("Failed to read: {}", e),
        },
    }
}

fn validate_judge_data<R: ReadMemory>(reader: &R, addr: u64) -> OffsetValidation {
    if addr == 0 {
        return OffsetValidation {
            name: "judgeData".to_string(),
            address: addr,
            valid: false,
            reason: "Address is zero".to_string(),
        };
    }

    // Check state markers at known offsets
    const STATE_MARKER_1: u64 = 0x5C;
    const STATE_MARKER_2: u64 = 0x60;

    let marker1 = reader.read_i32(addr + STATE_MARKER_1).unwrap_or(-1);
    let marker2 = reader.read_i32(addr + STATE_MARKER_2).unwrap_or(-1);

    if (0..=100).contains(&marker1) && (0..=100).contains(&marker2) {
        OffsetValidation {
            name: "judgeData".to_string(),
            address: addr,
            valid: true,
            reason: format!(
                "State markers valid: marker1={}, marker2={}",
                marker1, marker2
            ),
        }
    } else {
        OffsetValidation {
            name: "judgeData".to_string(),
            address: addr,
            valid: false,
            reason: format!(
                "Invalid state markers: marker1={}, marker2={}",
                marker1, marker2
            ),
        }
    }
}

fn validate_play_settings<R: ReadMemory>(reader: &R, addr: u64) -> OffsetValidation {
    if addr == 0 {
        return OffsetValidation {
            name: "playSettings".to_string(),
            address: addr,
            valid: false,
            reason: "Address is zero".to_string(),
        };
    }

    let style = reader.read_i32(addr).unwrap_or(-1);
    let gauge = reader.read_i32(addr + 4).unwrap_or(-1);
    let assist = reader.read_i32(addr + 8).unwrap_or(-1);
    let flip = reader.read_i32(addr + 12).unwrap_or(-1);
    let range = reader.read_i32(addr + 16).unwrap_or(-1);

    if (0..=6).contains(&style)
        && (0..=4).contains(&gauge)
        && (0..=5).contains(&assist)
        && (0..=1).contains(&flip)
        && (0..=5).contains(&range)
    {
        OffsetValidation {
            name: "playSettings".to_string(),
            address: addr,
            valid: true,
            reason: format!(
                "Valid: style={}, gauge={}, assist={}, flip={}, range={}",
                style, gauge, assist, flip, range
            ),
        }
    } else {
        OffsetValidation {
            name: "playSettings".to_string(),
            address: addr,
            valid: false,
            reason: format!(
                "Invalid values: style={}, gauge={}, assist={}, flip={}, range={}",
                style, gauge, assist, flip, range
            ),
        }
    }
}

fn validate_play_data<R: ReadMemory>(reader: &R, addr: u64) -> OffsetValidation {
    if addr == 0 {
        return OffsetValidation {
            name: "playData".to_string(),
            address: addr,
            valid: false,
            reason: "Address is zero".to_string(),
        };
    }

    let song_id = reader.read_i32(addr).unwrap_or(-1);
    let difficulty = reader.read_i32(addr + 4).unwrap_or(-1);
    let ex_score = reader.read_i32(addr + 8).unwrap_or(-1);
    let miss_count = reader.read_i32(addr + 12).unwrap_or(-1);

    // Accept initial state (all zeros)
    if song_id == 0 && difficulty == 0 && ex_score == 0 && miss_count == 0 {
        return OffsetValidation {
            name: "playData".to_string(),
            address: addr,
            valid: true,
            reason: "Initial state (all zeros)".to_string(),
        };
    }

    if (1000..=50000).contains(&song_id)
        && (0..=9).contains(&difficulty)
        && (0..=10000).contains(&ex_score)
        && (0..=3000).contains(&miss_count)
    {
        OffsetValidation {
            name: "playData".to_string(),
            address: addr,
            valid: true,
            reason: format!(
                "Valid: song_id={}, diff={}, ex_score={}, miss={}",
                song_id, difficulty, ex_score, miss_count
            ),
        }
    } else {
        OffsetValidation {
            name: "playData".to_string(),
            address: addr,
            valid: false,
            reason: format!(
                "Invalid: song_id={}, diff={}, ex_score={}, miss={}",
                song_id, difficulty, ex_score, miss_count
            ),
        }
    }
}

fn validate_current_song<R: ReadMemory>(reader: &R, addr: u64) -> OffsetValidation {
    if addr == 0 {
        return OffsetValidation {
            name: "currentSong".to_string(),
            address: addr,
            valid: false,
            reason: "Address is zero".to_string(),
        };
    }

    let song_id = reader.read_i32(addr).unwrap_or(-1);
    let difficulty = reader.read_i32(addr + 4).unwrap_or(-1);

    // Accept initial state
    if song_id == 0 && difficulty == 0 {
        return OffsetValidation {
            name: "currentSong".to_string(),
            address: addr,
            valid: true,
            reason: "Initial state (zeros)".to_string(),
        };
    }

    if (1000..=50000).contains(&song_id) && (0..=9).contains(&difficulty) {
        OffsetValidation {
            name: "currentSong".to_string(),
            address: addr,
            valid: true,
            reason: format!("Valid: song_id={}, difficulty={}", song_id, difficulty),
        }
    } else {
        OffsetValidation {
            name: "currentSong".to_string(),
            address: addr,
            valid: false,
            reason: format!("Invalid: song_id={}, difficulty={}", song_id, difficulty),
        }
    }
}

fn validate_data_map<R: ReadMemory>(reader: &R, addr: u64) -> OffsetValidation {
    if addr == 0 {
        return OffsetValidation {
            name: "dataMap".to_string(),
            address: addr,
            valid: false,
            reason: "Address is zero".to_string(),
        };
    }

    // DataMap structure: table_start at addr, table_end at addr+8
    let table_start = reader.read_u64(addr).unwrap_or(0);
    let table_end = reader.read_u64(addr + 8).unwrap_or(0);

    if table_end > table_start && table_end - table_start < 0x1000000 {
        let size = table_end - table_start;
        OffsetValidation {
            name: "dataMap".to_string(),
            address: addr,
            valid: true,
            reason: format!(
                "Table range valid: 0x{:X} - 0x{:X} ({} bytes)",
                table_start, table_end, size
            ),
        }
    } else {
        OffsetValidation {
            name: "dataMap".to_string(),
            address: addr,
            valid: false,
            reason: format!(
                "Invalid table range: start=0x{:X}, end=0x{:X}",
                table_start, table_end
            ),
        }
    }
}

fn validate_unlock_data<R: ReadMemory>(reader: &R, addr: u64) -> OffsetValidation {
    if addr == 0 {
        return OffsetValidation {
            name: "unlockData".to_string(),
            address: addr,
            valid: false,
            reason: "Address is zero".to_string(),
        };
    }

    // First unlock entry should have song_id around 1000, type=1, unlocks=462
    let song_id = reader.read_i32(addr).unwrap_or(-1);
    let unlock_type = reader.read_i32(addr + 4).unwrap_or(-1);
    let unlocks = reader.read_i32(addr + 8).unwrap_or(-1);

    if song_id == 1000 && unlock_type == 1 && unlocks == 462 {
        OffsetValidation {
            name: "unlockData".to_string(),
            address: addr,
            valid: true,
            reason: format!(
                "Valid: song_id={}, type={}, unlocks={}",
                song_id, unlock_type, unlocks
            ),
        }
    } else if (1000..=50000).contains(&song_id) {
        OffsetValidation {
            name: "unlockData".to_string(),
            address: addr,
            valid: true,
            reason: format!(
                "Plausible: song_id={}, type={}, unlocks={}",
                song_id, unlock_type, unlocks
            ),
        }
    } else {
        OffsetValidation {
            name: "unlockData".to_string(),
            address: addr,
            valid: false,
            reason: format!(
                "Invalid: song_id={}, type={}, unlocks={}",
                song_id, unlock_type, unlocks
            ),
        }
    }
}

fn count_songs_at_address<R: ReadMemory>(reader: &R, addr: u64) -> usize {
    if addr == 0 {
        return 0;
    }

    let mut count = 0;
    let mut consecutive_failures = 0;
    const MAX_SONGS: usize = 5000;
    const MAX_FAILURES: u32 = 10;

    let mut current_addr = addr;
    while count < MAX_SONGS && consecutive_failures < MAX_FAILURES {
        match SongInfo::read_from_memory(reader, current_addr) {
            Ok(Some(song)) if !song.title.is_empty() => {
                count += 1;
                consecutive_failures = 0;
            }
            _ => {
                consecutive_failures += 1;
            }
        }
        current_addr += SongInfo::MEMORY_SIZE as u64;
    }

    count
}

fn get_current_song_info<R: ReadMemory>(
    reader: &R,
    current_song_addr: u64,
    song_list_addr: u64,
) -> Option<CurrentSongInfo> {
    if current_song_addr == 0 {
        return None;
    }

    let song_id = reader.read_i32(current_song_addr).ok()?;
    let difficulty = reader.read_i32(current_song_addr + 4).ok()?;

    if song_id <= 0 || !(0..=9).contains(&difficulty) {
        return None;
    }

    // Try to get title from song database
    let title = if song_list_addr != 0 {
        // This is a simplified lookup - in practice we'd use the full song DB
        None
    } else {
        None
    };

    Some(CurrentSongInfo {
        song_id: song_id as u32,
        difficulty: difficulty as u8,
        title,
    })
}
