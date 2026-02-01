//! Song database scanning utilities

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use crate::chart::SongInfo;
use crate::process::ReadMemory;

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

    // Entry structure:
    // - text_entry[i] = song_list_addr + i * ENTRY_SIZE
    // - meta_entry[i] = song_list_addr + METADATA_TABLE_OFFSET + i * ENTRY_SIZE
    const ENTRY_SIZE: u64 = SongInfo::MEMORY_SIZE as u64; // 0x3F0 = 1008 bytes
    const METADATA_OFFSET: u64 = SongInfo::METADATA_TABLE_OFFSET as u64; // 0x7E0 = 2016 bytes

    let max_entries = (scan_range as u64 / ENTRY_SIZE).min(5000);

    // Note: With lazy loading, songs may be scattered across the entry table.
    // We scan all entries without early termination to find all loaded songs.
    // Approach: first check if title exists, then read metadata.
    for i in 0..max_entries {
        let text_addr = song_list_addr + i * ENTRY_SIZE;
        let meta_addr = text_addr + METADATA_OFFSET;

        // First, check if title exists at this entry
        let title = match reader.read_bytes(text_addr, 64) {
            Ok(title_bytes) => {
                let len = title_bytes.iter().position(|&b| b == 0).unwrap_or(64);
                if len == 0 {
                    continue;
                }
                let (decoded, _, _) = SHIFT_JIS.decode(&title_bytes[..len]);
                let title = decoded.trim();
                if title.is_empty()
                    || !title
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_graphic() || !c.is_ascii())
                {
                    continue;
                }
                title.to_string()
            }
            Err(_) => continue,
        };

        // Read metadata for this entry
        let Ok(meta_bytes) = reader.read_bytes(meta_addr, 20) else {
            continue;
        };

        let song_id =
            i32::from_le_bytes([meta_bytes[0], meta_bytes[1], meta_bytes[2], meta_bytes[3]]);
        let folder =
            i32::from_le_bytes([meta_bytes[4], meta_bytes[5], meta_bytes[6], meta_bytes[7]]);

        // Validate song_id and folder ranges
        // Note: folder values vary widely in new INFINITAS versions (e.g., 1-200+)
        if !(1000..=90000).contains(&song_id) || !(1..=200).contains(&folder) {
            continue;
        }

        // Skip if we already have this song_id
        if songs.iter().any(|s| s.song_id == song_id as u32) {
            continue;
        }

        // Parse levels from difficulty ASCII (offset 8 in metadata)
        let mut levels = [0u8; 10];
        for (j, &byte) in meta_bytes[8..18].iter().enumerate() {
            if byte.is_ascii_digit() {
                levels[j] = byte - b'0';
            }
        }

        songs.push(ScannedSong {
            song_id: song_id as u32,
            title,
            folder,
            levels,
            source_offset: meta_addr,
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
