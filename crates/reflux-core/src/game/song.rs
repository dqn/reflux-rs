use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::error::Result;
use crate::game::UnlockType;
use crate::memory::MemoryReader;

/// Song metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SongInfo {
    pub id: u32,
    pub title: String,
    pub title_english: String,
    pub artist: String,
    pub genre: String,
    pub bpm: String,
    pub folder: i32,
    /// Level for each difficulty: SPB, SPN, SPH, SPA, SPL, DPB, DPN, DPH, DPA, DPL
    pub levels: [u8; 10],
    /// Total notes for each difficulty
    pub total_notes: [u32; 10],
    pub unlock_type: UnlockType,
}

impl SongInfo {
    /// Size of one song entry in memory (0x3F0 = 1008 bytes)
    /// This corresponds to the INFINITAS internal song entry structure.
    pub const MEMORY_SIZE: usize = 0x3F0;

    // Memory layout constants
    // INFINITAS stores song metadata in fixed-size blocks with the following layout:
    const SLAB: usize = 64; // String block size (64 bytes per Shift-JIS string field)
    const WORD: usize = 4; // i32/u32 size

    // Memory offsets (relative to song entry start)
    // String fields (each 64 bytes, Shift-JIS encoded):
    //   0x000: Title
    //   0x040: Title (English)
    //   0x080: Genre
    //   0x0C0: Artist
    const TITLE_OFFSET: usize = 0;
    const TITLE_ENGLISH_OFFSET: usize = Self::SLAB; // 64
    const GENRE_OFFSET: usize = Self::SLAB * 2; // 128
    const ARTIST_OFFSET: usize = Self::SLAB * 3; // 192

    // Metadata section (starts at 0x100 = 256):
    const FOLDER_OFFSET: usize = Self::SLAB * 4 + 24; // 280
    const LEVELS_OFFSET: usize = Self::SLAB * 4 + Self::SLAB / 2; // 288 (10 bytes)
    const BPM_OFFSET: usize = Self::SLAB * 5; // 320 (8 bytes: max, min)
    const NOTES_OFFSET: usize = Self::SLAB * 6 + 48; // 432 (40 bytes: 10 x i32)
    const SONG_ID_OFFSET: usize = 256 + 368; // 624

    /// Get level for a specific difficulty index
    pub fn get_level(&self, difficulty_index: usize) -> u8 {
        self.levels.get(difficulty_index).copied().unwrap_or(0)
    }

    /// Get total notes for a specific difficulty index
    pub fn get_total_notes(&self, difficulty_index: usize) -> u32 {
        self.total_notes.get(difficulty_index).copied().unwrap_or(0)
    }

    /// Read song info from memory at the given address
    pub fn read_from_memory(reader: &MemoryReader, address: u64) -> Result<Option<Self>> {
        // Read entire song block
        let buffer = reader.read_bytes(address, Self::MEMORY_SIZE)?;

        // Check if entry is valid (first 4 bytes should not be 0)
        if buffer[0..4] == [0, 0, 0, 0] {
            return Ok(None);
        }

        // Parse strings (Shift-JIS encoded)
        let title = decode_shift_jis(&buffer[Self::TITLE_OFFSET..Self::TITLE_ENGLISH_OFFSET]);
        let title_english =
            decode_shift_jis(&buffer[Self::TITLE_ENGLISH_OFFSET..Self::GENRE_OFFSET]);
        let genre = decode_shift_jis(&buffer[Self::GENRE_OFFSET..Self::ARTIST_OFFSET]);
        let artist =
            decode_shift_jis(&buffer[Self::ARTIST_OFFSET..Self::ARTIST_OFFSET + Self::SLAB]);

        // Parse folder (1 byte)
        let folder = buffer[Self::FOLDER_OFFSET] as i32;

        // Parse difficulty levels (10 bytes)
        let mut levels = [0u8; 10];
        levels.copy_from_slice(&buffer[Self::LEVELS_OFFSET..Self::LEVELS_OFFSET + 10]);

        // Parse BPM (8 bytes: max, min)
        let bpm_max = i32::from_le_bytes([
            buffer[Self::BPM_OFFSET],
            buffer[Self::BPM_OFFSET + 1],
            buffer[Self::BPM_OFFSET + 2],
            buffer[Self::BPM_OFFSET + 3],
        ]);
        let bpm_min = i32::from_le_bytes([
            buffer[Self::BPM_OFFSET + Self::WORD],
            buffer[Self::BPM_OFFSET + Self::WORD + 1],
            buffer[Self::BPM_OFFSET + Self::WORD + 2],
            buffer[Self::BPM_OFFSET + Self::WORD + 3],
        ]);

        let bpm = if bpm_min != 0 && bpm_min != bpm_max {
            format!("{:03}~{:03}", bpm_min, bpm_max)
        } else {
            format!("{:03}", bpm_max)
        };

        // Parse note counts (40 bytes = 10 x i32)
        let mut total_notes = [0u32; 10];
        for (i, note_count) in total_notes.iter_mut().enumerate() {
            let offset = Self::NOTES_OFFSET + i * Self::WORD;
            *note_count = u32::from_le_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
            ]);
        }

        // Parse song ID (4 bytes)
        let song_id = i32::from_le_bytes([
            buffer[Self::SONG_ID_OFFSET],
            buffer[Self::SONG_ID_OFFSET + 1],
            buffer[Self::SONG_ID_OFFSET + 2],
            buffer[Self::SONG_ID_OFFSET + 3],
        ]);

        Ok(Some(SongInfo {
            id: song_id as u32,
            title,
            title_english,
            artist,
            genre,
            bpm,
            folder,
            levels,
            total_notes,
            unlock_type: UnlockType::default(),
        }))
    }
}

/// Decode Shift-JIS bytes to String, removing null terminators
fn decode_shift_jis(bytes: &[u8]) -> String {
    use encoding_rs::SHIFT_JIS;

    // Find null terminator
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let bytes = &bytes[..len];

    let (decoded, _, had_errors) = SHIFT_JIS.decode(bytes);
    if had_errors {
        warn!(
            "Shift-JIS decoding had errors for bytes: {:?}",
            &bytes[..bytes.len().min(20)]
        );
    }
    decoded.into_owned()
}

/// Fetch entire song database from memory
pub fn fetch_song_database(
    reader: &MemoryReader,
    song_list_addr: u64,
) -> Result<HashMap<u32, SongInfo>> {
    let mut result = HashMap::new();
    let mut current_position: u64 = 0;

    loop {
        let address = song_list_addr + current_position;

        match SongInfo::read_from_memory(reader, address)? {
            Some(song) if !song.title.is_empty() => {
                result.insert(song.id, song);
            }
            _ => {
                // End of song list
                break;
            }
        }

        current_position += SongInfo::MEMORY_SIZE as u64;
    }

    Ok(result)
}
