//! Song database scanning utilities

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use crate::game::SongInfo;
use crate::memory::ReadMemory;

/// Information about a scanned song
#[derive(Debug, Clone, Serialize)]
pub struct ScannedSong {
    pub song_id: u32,
    pub title: String,
    pub folder: i32,
    pub levels: [u8; 10],
    pub source_offset: u64,
    pub source_type: String,
}

/// TSV matching result
#[derive(Debug, Clone, Serialize)]
pub struct TsvMatch {
    pub song_id: u32,
    pub memory_title: String,
    pub tsv_title: Option<String>,
    pub matched: bool,
}

/// Result of a song database scan
#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    /// Scan start address
    pub scan_start: u64,
    /// Scan range in bytes
    pub scan_range: usize,
    /// Number of songs found
    pub songs_found: usize,
    /// List of scanned songs
    pub songs: Vec<ScannedSong>,
    /// TSV matching results (if TSV was provided)
    pub tsv_matches: Option<Vec<TsvMatch>>,
    /// Number of matched songs
    pub matched_count: Option<usize>,
    /// Number of unmatched songs
    pub unmatched_count: Option<usize>,
}

impl ScanResult {
    /// Perform a comprehensive scan for song data
    pub fn scan<R: ReadMemory>(
        reader: &R,
        song_list_addr: u64,
        scan_range: usize,
        tsv_titles: Option<&HashMap<Arc<str>, SongInfo>>,
    ) -> Self {
        let mut songs = Vec::new();

        // Method 1: Scan from text table (old method)
        let text_songs = scan_text_table(reader, song_list_addr, scan_range);
        songs.extend(text_songs);

        // Method 2: Scan metadata table
        let metadata_songs = scan_metadata_table(reader, song_list_addr, scan_range);
        for song in metadata_songs {
            // Avoid duplicates by song_id
            if !songs.iter().any(|s| s.song_id == song.song_id) {
                songs.push(song);
            }
        }

        // Sort by song_id
        songs.sort_by_key(|s| s.song_id);

        let songs_found = songs.len();

        // TSV matching
        let (tsv_matches, matched_count, unmatched_count) = if let Some(tsv) = tsv_titles {
            let matches = compute_tsv_matches(&songs, tsv);
            let matched = matches.iter().filter(|m| m.matched).count();
            let unmatched = matches.iter().filter(|m| !m.matched).count();
            (Some(matches), Some(matched), Some(unmatched))
        } else {
            (None, None, None)
        };

        ScanResult {
            scan_start: song_list_addr,
            scan_range,
            songs_found,
            songs,
            tsv_matches,
            matched_count,
            unmatched_count,
        }
    }
}

fn scan_text_table<R: ReadMemory>(
    reader: &R,
    song_list_addr: u64,
    _scan_range: usize,
) -> Vec<ScannedSong> {
    let mut songs = Vec::new();

    if song_list_addr == 0 {
        return songs;
    }

    let mut consecutive_failures = 0;
    const MAX_FAILURES: u32 = 10;

    for i in 0..5000u64 {
        let entry_addr = song_list_addr + i * SongInfo::MEMORY_SIZE as u64;

        match SongInfo::read_from_memory(reader, entry_addr) {
            Ok(Some(song)) if !song.title.is_empty() && song.id > 0 => {
                songs.push(ScannedSong {
                    song_id: song.id,
                    title: song.title.to_string(),
                    folder: song.folder,
                    levels: song.levels,
                    source_offset: entry_addr,
                    source_type: "text_table".to_string(),
                });
                consecutive_failures = 0;
            }
            _ => {
                consecutive_failures += 1;
                if consecutive_failures >= MAX_FAILURES {
                    break;
                }
            }
        }
    }

    songs
}

fn scan_metadata_table<R: ReadMemory>(
    reader: &R,
    song_list_addr: u64,
    scan_range: usize,
) -> Vec<ScannedSong> {
    use encoding_rs::SHIFT_JIS;

    let mut songs = Vec::new();

    if song_list_addr == 0 {
        return songs;
    }

    let metadata_base = song_list_addr + SongInfo::METADATA_TABLE_OFFSET as u64;

    // Read metadata area
    let Ok(buffer) = reader.read_bytes(metadata_base, scan_range) else {
        return songs;
    };

    // Scan for valid (song_id, folder) pairs
    for offset in (0..buffer.len().saturating_sub(32)).step_by(4) {
        let song_id = i32::from_le_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]);
        let folder = i32::from_le_bytes([
            buffer[offset + 4],
            buffer[offset + 5],
            buffer[offset + 6],
            buffer[offset + 7],
        ]);

        // Validate song_id and folder
        if song_id < 1000 || song_id > 50000 || folder < 1 || folder > 50 {
            continue;
        }

        // Skip if we already have this song_id
        if songs.iter().any(|s| s.song_id == song_id as u32) {
            continue;
        }

        // Calculate title address: metadata_addr - 0x7E0
        let metadata_addr = metadata_base + offset as u64;
        let title_addr = metadata_addr.saturating_sub(SongInfo::METADATA_TABLE_OFFSET as u64);

        // Read title
        let title = if let Ok(title_bytes) = reader.read_bytes(title_addr, 64) {
            let len = title_bytes.iter().position(|&b| b == 0).unwrap_or(64);
            if len > 0 {
                let (decoded, _, _) = SHIFT_JIS.decode(&title_bytes[..len]);
                let title = decoded.trim();
                if !title.is_empty()
                    && title
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_graphic() || !c.is_ascii())
                        .unwrap_or(false)
                {
                    title.to_string()
                } else {
                    continue;
                }
            } else {
                continue;
            }
        } else {
            continue;
        };

        // Parse levels (ASCII at offset 8)
        let mut levels = [0u8; 10];
        if offset + 18 <= buffer.len() {
            for (i, &byte) in buffer[offset + 8..offset + 18].iter().enumerate() {
                if byte >= b'0' && byte <= b'9' {
                    levels[i] = byte - b'0';
                }
            }
        }

        songs.push(ScannedSong {
            song_id: song_id as u32,
            title,
            folder,
            levels,
            source_offset: metadata_addr,
            source_type: "metadata_table".to_string(),
        });
    }

    songs
}

fn compute_tsv_matches(songs: &[ScannedSong], tsv: &HashMap<Arc<str>, SongInfo>) -> Vec<TsvMatch> {
    let mut matches = Vec::new();

    for song in songs {
        let normalized_title = normalize_title(&song.title);

        // Try exact match first
        let tsv_match = tsv.get(&Arc::from(song.title.as_str())).or_else(|| {
            // Try normalized match
            tsv.iter()
                .find(|(k, _)| normalize_title(k) == normalized_title)
                .map(|(_, v)| v)
        });

        matches.push(TsvMatch {
            song_id: song.song_id,
            memory_title: song.title.clone(),
            tsv_title: tsv_match.map(|s| s.title.to_string()),
            matched: tsv_match.is_some(),
        });
    }

    matches
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect()
}
