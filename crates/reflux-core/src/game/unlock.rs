use std::collections::HashMap;

use crate::error::Result;
use crate::game::{Difficulty, SongInfo, UnlockType};
use crate::memory::MemoryReader;

/// Unlock data structure from memory
#[derive(Debug, Clone, Default)]
pub struct UnlockData {
    pub song_id: i32,
    pub unlock_type: UnlockType,
    pub unlocks: i32, // Bitmask of unlocked difficulties
}

impl UnlockData {
    /// Size of unlock data structure in memory (32 bytes)
    pub const MEMORY_SIZE: usize = 32;

    /// Check if a specific difficulty is unlocked (raw bit check)
    pub fn is_difficulty_unlocked(&self, difficulty: Difficulty) -> bool {
        let bit = 1 << (difficulty as i32);
        (self.unlocks & bit) != 0
    }

    /// Parse from raw bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::MEMORY_SIZE {
            return None;
        }

        let song_id = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let unlock_type_val = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let unlocks = i32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

        let unlock_type = match unlock_type_val {
            1 => UnlockType::Base,
            2 => UnlockType::Bits,
            3 => UnlockType::Sub,
            _ => UnlockType::Base,
        };

        Some(Self {
            song_id,
            unlock_type,
            unlocks,
        })
    }
}

/// Load unlock states from memory for all songs
pub fn get_unlock_states(
    reader: &MemoryReader,
    unlock_data_addr: u64,
    song_db: &HashMap<String, SongInfo>,
) -> Result<HashMap<String, UnlockData>> {
    let mut result = HashMap::new();

    let song_count = song_db.len();
    if song_count == 0 {
        return Ok(result);
    }

    let mut position_entries = 0usize;
    let mut batch_entries = song_count;

    loop {
        let buffer_size = UnlockData::MEMORY_SIZE * batch_entries;
        let buffer = reader.read_bytes(
            unlock_data_addr + (position_entries * UnlockData::MEMORY_SIZE) as u64,
            buffer_size,
        )?;

        let extra_entries = parse_unlock_buffer(&buffer, song_db, &mut result);
        if extra_entries == 0 {
            break;
        }

        position_entries += batch_entries;
        batch_entries = extra_entries;
    }

    Ok(result)
}

fn parse_unlock_buffer(
    buffer: &[u8],
    song_db: &HashMap<String, SongInfo>,
    result: &mut HashMap<String, UnlockData>,
) -> usize {
    let mut position = 0;
    let mut extra_entries = 0;

    while position + UnlockData::MEMORY_SIZE <= buffer.len() {
        let chunk = &buffer[position..position + UnlockData::MEMORY_SIZE];

        if let Some(data) = UnlockData::from_bytes(chunk) {
            if data.song_id == 0 {
                break;
            }

            let song_id = format!("{:05}", data.song_id);
            if !song_db.contains_key(&song_id) {
                extra_entries += 1;
            }
            result.insert(song_id, data);
        }

        position += UnlockData::MEMORY_SIZE;
    }

    extra_entries
}

/// Get unlock state for a specific difficulty, considering special cases
///
/// Special handling for:
/// - SPB (Beginner): For non-Sub songs, check if note count is non-zero
/// - SPL/DPL (Leggendaria): For Sub songs, requires both SPA and DPA to be unlocked
pub fn get_unlock_state_for_difficulty(
    unlock_db: &HashMap<String, UnlockData>,
    song_db: &HashMap<String, SongInfo>,
    song_id: &str,
    difficulty: Difficulty,
) -> bool {
    let Some(unlock_data) = unlock_db.get(song_id) else {
        return false;
    };

    let song_info = song_db.get(song_id);

    // Handle Beginner difficulty specially
    if difficulty == Difficulty::SpB {
        if unlock_data.unlock_type == UnlockType::Sub {
            // For Sub songs, use the unlock bit
            return unlock_data.is_difficulty_unlocked(difficulty);
        } else {
            // For other songs, check if note count is non-zero
            return song_info.map(|s| s.total_notes[0] > 0).unwrap_or(false);
        }
    }

    // Handle Leggendaria difficulties (SPL/DPL)
    if difficulty == Difficulty::SpL || difficulty == Difficulty::DpL {
        if unlock_data.unlock_type == UnlockType::Sub {
            // For Sub songs, require both SPA and DPA to be unlocked
            let spa_unlocked = unlock_data.is_difficulty_unlocked(Difficulty::SpA);
            let dpa_unlocked = unlock_data.is_difficulty_unlocked(Difficulty::DpA);
            return spa_unlocked && dpa_unlocked;
        } else {
            // For other songs, just check the unlock bit
            return unlock_data.is_difficulty_unlocked(difficulty);
        }
    }

    // Standard case: just check the unlock bit
    unlock_data.is_difficulty_unlocked(difficulty)
}

/// Compare old and new unlock states and return only changed entries
///
/// This function:
/// 1. Reads current unlock states from memory
/// 2. Compares with previous states
/// 3. Returns only entries where `unlocks` value has changed
pub fn update_unlock_states(
    reader: &MemoryReader,
    old_state: &HashMap<String, UnlockData>,
    unlock_data_addr: u64,
    song_db: &HashMap<String, SongInfo>,
) -> Result<HashMap<String, UnlockData>> {
    // Get current state from memory
    let current_state = get_unlock_states(reader, unlock_data_addr, song_db)?;

    let mut changes = HashMap::new();

    for (song_id, current_data) in &current_state {
        if let Some(old_data) = old_state.get(song_id) {
            // Check if unlock state changed
            if current_data.unlocks != old_data.unlocks {
                changes.insert(song_id.clone(), current_data.clone());
            }
        }
        // Note: New songs not in old_state are not considered "changes"
        // They should be handled by the server sync logic
    }

    Ok(changes)
}

/// Detect unlock state changes without re-reading from memory
/// (for use when you already have the new state)
pub fn detect_unlock_changes(
    old_state: &HashMap<String, UnlockData>,
    new_state: &HashMap<String, UnlockData>,
) -> HashMap<String, UnlockData> {
    let mut changes = HashMap::new();

    for (song_id, new_data) in new_state {
        if let Some(old_data) = old_state.get(song_id)
            && new_data.unlocks != old_data.unlocks
        {
            changes.insert(song_id.clone(), new_data.clone());
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_difficulty_unlocked() {
        let unlock = UnlockData {
            song_id: 1000,
            unlock_type: UnlockType::Base,
            unlocks: 0b11111, // SPB through SPL unlocked
        };

        assert!(unlock.is_difficulty_unlocked(Difficulty::SpB));
        assert!(unlock.is_difficulty_unlocked(Difficulty::SpN));
        assert!(unlock.is_difficulty_unlocked(Difficulty::SpH));
        assert!(unlock.is_difficulty_unlocked(Difficulty::SpA));
        assert!(unlock.is_difficulty_unlocked(Difficulty::SpL));
        assert!(!unlock.is_difficulty_unlocked(Difficulty::DpN));
    }

    #[test]
    fn test_from_bytes() {
        let bytes = [
            0xE8, 0x03, 0x00, 0x00, // song_id = 1000
            0x01, 0x00, 0x00, 0x00, // unlock_type = Base
            0x1F, 0x00, 0x00, 0x00, // unlocks = 0x1F
            0x00, 0x00, 0x00, 0x00, // padding
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];

        let unlock = UnlockData::from_bytes(&bytes).unwrap();
        assert_eq!(unlock.song_id, 1000);
        assert_eq!(unlock.unlock_type, UnlockType::Base);
        assert_eq!(unlock.unlocks, 0x1F);
    }
}
