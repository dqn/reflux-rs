//! Memory dump utilities for debugging

use serde::Serialize;

use crate::game::SongInfo;
use crate::memory::ReadMemory;
use crate::offset::OffsetsCollection;

/// Memory dump at a specific location
#[derive(Debug, Clone, Serialize)]
pub struct MemoryDump {
    pub address: u64,
    pub size: usize,
    #[serde(skip_serializing)]
    pub bytes: Vec<u8>,
    pub hex_dump: Vec<String>,
}

impl MemoryDump {
    /// Create a new memory dump from raw bytes
    pub fn new(address: u64, bytes: Vec<u8>) -> Self {
        let hex_dump = format_hex_dump(address, &bytes);
        Self {
            address,
            size: bytes.len(),
            bytes,
            hex_dump,
        }
    }
}

/// Complete dump information
#[derive(Debug, Clone, Serialize)]
pub struct DumpInfo {
    /// Offsets collection
    pub offsets: OffsetsCollection,
    /// Memory dump around songList
    pub song_list_dump: Option<MemoryDump>,
    /// First few song entries
    pub song_entries: Vec<SongEntryDump>,
    /// Metadata table sample
    pub metadata_sample: Option<MemoryDump>,
    /// Detected songs list
    pub detected_songs: Vec<DetectedSong>,
}

/// Dump of a single song entry
#[derive(Debug, Clone, Serialize)]
pub struct SongEntryDump {
    pub index: usize,
    pub address: u64,
    pub song_id: i32,
    pub folder: i32,
    pub title: String,
    pub levels: [u8; 10],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_song_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_folder: Option<i32>,
}

/// Information about a detected song
#[derive(Debug, Clone, Serialize)]
pub struct DetectedSong {
    pub song_id: u32,
    pub title: String,
    pub folder: i32,
    pub source: String,
}

impl DumpInfo {
    /// Collect dump information from the game process
    pub fn collect<R: ReadMemory>(reader: &R, offsets: &OffsetsCollection) -> Self {
        let song_list_dump = dump_song_list_area(reader, offsets.song_list);
        let song_entries = dump_song_entries(reader, offsets.song_list, 10);
        let metadata_sample = dump_metadata_table(reader, offsets.song_list);
        let detected_songs = collect_detected_songs(reader, offsets.song_list);

        DumpInfo {
            offsets: offsets.clone(),
            song_list_dump,
            song_entries,
            metadata_sample,
            detected_songs,
        }
    }
}

fn dump_song_list_area<R: ReadMemory>(reader: &R, addr: u64) -> Option<MemoryDump> {
    if addr == 0 {
        return None;
    }

    // Dump 256 bytes around the songList address
    let dump_size = 256;
    match reader.read_bytes(addr, dump_size) {
        Ok(bytes) => Some(MemoryDump::new(addr, bytes)),
        Err(_) => None,
    }
}

fn dump_song_entries<R: ReadMemory>(
    reader: &R,
    song_list_addr: u64,
    count: usize,
) -> Vec<SongEntryDump> {
    if song_list_addr == 0 {
        return Vec::new();
    }

    let mut entries = Vec::new();
    let metadata_base = song_list_addr + SongInfo::METADATA_TABLE_OFFSET as u64;

    for i in 0..count {
        let entry_addr = song_list_addr + i as u64 * SongInfo::MEMORY_SIZE as u64;
        let metadata_addr = metadata_base + i as u64 * SongInfo::MEMORY_SIZE as u64;

        let (song_id, folder, title, levels) = match reader.read_bytes(entry_addr, SongInfo::MEMORY_SIZE) {
            Ok(bytes) => {
                // Parse title (first 64 bytes, Shift-JIS)
                let title = decode_shift_jis(&bytes[0..64]);

                // Parse song_id and folder from main entry
                let song_id = i32::from_le_bytes([bytes[624], bytes[625], bytes[626], bytes[627]]);
                let folder = bytes[280] as i32;

                // Parse levels
                let mut levels = [0u8; 10];
                levels.copy_from_slice(&bytes[288..298]);

                (song_id, folder, title, levels)
            }
            Err(_) => continue,
        };

        // Try to read from metadata table
        let (metadata_song_id, metadata_folder) = match reader.read_bytes(metadata_addr, 8) {
            Ok(bytes) => {
                let meta_id = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let meta_folder = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                (Some(meta_id), Some(meta_folder))
            }
            Err(_) => (None, None),
        };

        entries.push(SongEntryDump {
            index: i,
            address: entry_addr,
            song_id,
            folder,
            title,
            levels,
            metadata_song_id,
            metadata_folder,
        });
    }

    entries
}

fn dump_metadata_table<R: ReadMemory>(reader: &R, song_list_addr: u64) -> Option<MemoryDump> {
    if song_list_addr == 0 {
        return None;
    }

    let metadata_addr = song_list_addr + SongInfo::METADATA_TABLE_OFFSET as u64;
    let dump_size = 256;

    match reader.read_bytes(metadata_addr, dump_size) {
        Ok(bytes) => Some(MemoryDump::new(metadata_addr, bytes)),
        Err(_) => None,
    }
}

fn collect_detected_songs<R: ReadMemory>(reader: &R, song_list_addr: u64) -> Vec<DetectedSong> {
    if song_list_addr == 0 {
        return Vec::new();
    }

    let mut songs = Vec::new();
    let metadata_base = song_list_addr + SongInfo::METADATA_TABLE_OFFSET as u64;

    // Scan up to 5000 entries
    for i in 0..5000u64 {
        let entry_addr = song_list_addr + i * SongInfo::MEMORY_SIZE as u64;
        let metadata_addr = metadata_base + i * SongInfo::MEMORY_SIZE as u64;

        // Try to read from main entry first
        if let Ok(bytes) = reader.read_bytes(entry_addr, 64) {
            let title = decode_shift_jis(&bytes);
            if !title.is_empty() {
                // Read song_id from main entry
                if let Ok(id_bytes) = reader.read_bytes(entry_addr + 624, 4) {
                    let song_id = i32::from_le_bytes([id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]]);
                    let folder = reader.read_i32(entry_addr + 280).unwrap_or(0);

                    if song_id > 0 {
                        songs.push(DetectedSong {
                            song_id: song_id as u32,
                            title: title.clone(),
                            folder,
                            source: "main_entry".to_string(),
                        });
                        continue;
                    }
                }

                // Try metadata table
                if let Ok(meta_bytes) = reader.read_bytes(metadata_addr, 8) {
                    let meta_id = i32::from_le_bytes([meta_bytes[0], meta_bytes[1], meta_bytes[2], meta_bytes[3]]);
                    let meta_folder = i32::from_le_bytes([meta_bytes[4], meta_bytes[5], meta_bytes[6], meta_bytes[7]]);

                    if meta_id >= 1000 && meta_id <= 50000 {
                        songs.push(DetectedSong {
                            song_id: meta_id as u32,
                            title,
                            folder: meta_folder,
                            source: "metadata_table".to_string(),
                        });
                    }
                }
            }
        }

        // Stop after 10 consecutive empty entries
        if songs.is_empty() && i > 10 {
            break;
        }
    }

    songs
}

fn format_hex_dump(address: u64, bytes: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let bytes_per_line = 16;

    for (i, chunk) in bytes.chunks(bytes_per_line).enumerate() {
        let addr = address + (i * bytes_per_line) as u64;
        let hex_part: String = chunk
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");

        let ascii_part: String = chunk
            .iter()
            .map(|&b| if b >= 0x20 && b < 0x7F { b as char } else { '.' })
            .collect();

        lines.push(format!("{:016X}  {:48}  {}", addr, hex_part, ascii_part));
    }

    lines
}

fn decode_shift_jis(bytes: &[u8]) -> String {
    use encoding_rs::SHIFT_JIS;
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let (decoded, _, _) = SHIFT_JIS.decode(&bytes[..len]);
    decoded.trim().to_string()
}
