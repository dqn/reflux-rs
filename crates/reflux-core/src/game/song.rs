use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::Result;
use crate::game::{EncodingFixes, UnlockType};
use crate::memory::ReadMemory;

/// Song metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SongInfo {
    pub id: u32,
    pub title: Arc<str>,
    pub title_english: Arc<str>,
    pub artist: Arc<str>,
    pub genre: Arc<str>,
    pub bpm: Arc<str>,
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

    /// Offset from text table to metadata table in new INFINITAS versions
    /// In version 2026012800+, the song_id is stored in a separate metadata table
    /// located 0x7E0 (2016) bytes after the text table base.
    pub const METADATA_TABLE_OFFSET: usize = 0x7E0;

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
    pub fn read_from_memory<R: ReadMemory>(reader: &R, address: u64) -> Result<Option<Self>> {
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

        let bpm: Arc<str> = if bpm_min != 0 && bpm_min != bpm_max {
            format!("{:03}~{:03}", bpm_min, bpm_max).into()
        } else {
            format!("{:03}", bpm_max).into()
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

    /// Read song info with fallback to metadata table for new INFINITAS versions.
    ///
    /// In version 2026012800+, the song_id may be stored in a separate metadata table.
    /// This method tries the standard read first, and if song_id is 0 but title exists,
    /// it attempts to read song_id from the metadata table.
    ///
    /// # Arguments
    /// * `reader` - Memory reader
    /// * `text_address` - Address of the text entry
    /// * `text_base` - Base address of the text table
    /// * `entry_index` - Index of this entry in the table
    pub fn read_from_memory_with_fallback<R: ReadMemory>(
        reader: &R,
        text_address: u64,
        text_base: u64,
        entry_index: u64,
    ) -> Result<Option<Self>> {
        // First, try standard read
        let result = Self::read_from_memory(reader, text_address)?;

        match result {
            Some(mut song) if song.id == 0 && !song.title.is_empty() => {
                // Try to read song_id from metadata table
                let metadata_addr =
                    text_base + Self::METADATA_TABLE_OFFSET as u64 + entry_index * Self::MEMORY_SIZE as u64;

                if let Ok(metadata) = reader.read_bytes(metadata_addr, 32) {
                    let alt_song_id = i32::from_le_bytes([
                        metadata[0], metadata[1], metadata[2], metadata[3],
                    ]);
                    let alt_folder = i32::from_le_bytes([
                        metadata[4], metadata[5], metadata[6], metadata[7],
                    ]);

                    // Validate: song_id should be 1000-50000, folder 1-50
                    if alt_song_id >= 1000 && alt_song_id <= 50000 {
                        debug!(
                            "Using metadata table for song '{}': id={}, folder={}",
                            song.title, alt_song_id, alt_folder
                        );
                        song.id = alt_song_id as u32;
                        if alt_folder >= 1 && alt_folder <= 50 {
                            song.folder = alt_folder;
                        }
                    }
                }

                if song.id == 0 {
                    // Still no valid song_id, skip this entry
                    debug!(
                        "Skipping entry with title '{}' - no valid song_id found",
                        song.title
                    );
                    return Ok(None);
                }

                Ok(Some(song))
            }
            other => Ok(other),
        }
    }
}

/// Decode Shift-JIS bytes to Arc<str>, removing null terminators
fn decode_shift_jis(bytes: &[u8]) -> Arc<str> {
    use encoding_rs::SHIFT_JIS;

    // Find null terminator
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let bytes = &bytes[..len];

    let (decoded, _, had_errors) = SHIFT_JIS.decode(bytes);
    if had_errors {
        debug!(
            "Shift-JIS decoding had errors for bytes: {:?}",
            &bytes[..bytes.len().min(20)]
        );
    }
    Arc::from(decoded.into_owned())
}

/// Analyze metadata table structure for new INFINITAS versions
///
/// This function scans the metadata table to find valid song_ids and determine
/// the actual entry size used by the new version.
pub fn analyze_metadata_table<R: ReadMemory>(reader: &R, text_base: u64) {
    let metadata_base = text_base + SongInfo::METADATA_TABLE_OFFSET as u64;
    info!("=== Metadata Table Analysis at 0x{:X} ===", metadata_base);

    // Read a large chunk to analyze
    let Ok(buffer) = reader.read_bytes(metadata_base, 0x10000) else {
        warn!("Failed to read metadata table");
        return;
    };

    // Scan for valid song_ids (pattern: 1000-50000 followed by reasonable folder 1-50)
    let mut found_ids: Vec<(usize, i32, i32)> = Vec::new();

    for offset in (0..buffer.len() - 8).step_by(4) {
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

        if song_id >= 1000 && song_id <= 50000 && folder >= 1 && folder <= 50 {
            found_ids.push((offset, song_id, folder));
        }
    }

    info!("Found {} potential song entries", found_ids.len());

    // Analyze spacing between entries
    if found_ids.len() >= 2 {
        let mut deltas: Vec<usize> = Vec::new();
        for i in 1..found_ids.len().min(20) {
            let delta = found_ids[i].0 - found_ids[i - 1].0;
            deltas.push(delta);
        }

        info!("Entry spacing (first 20): {:?}", deltas);

        // Find most common delta
        let mut delta_counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for d in &deltas {
            *delta_counts.entry(*d).or_insert(0) += 1;
        }
        if let Some((most_common, count)) = delta_counts.iter().max_by_key(|(_, v)| *v) {
            info!("Most common entry size: 0x{:X} ({} bytes), {} occurrences", most_common, most_common, count);
        }
    }

    // Show first 10 entries
    for (i, (offset, song_id, folder)) in found_ids.iter().take(10).enumerate() {
        let abs_addr = metadata_base + *offset as u64;
        debug!(
            "  Entry {}: song_id={}, folder={} at 0x{:X} (offset 0x{:X})",
            i, song_id, folder, abs_addr, offset
        );

        // Show bytes around this entry
        if offset + 32 <= buffer.len() {
            debug!("    Bytes: {:02X?}", &buffer[*offset..*offset + 32]);
        }
    }
}

/// Build a song_id to title mapping by scanning metadata table
///
/// For new INFINITAS versions (2026012800+), the title is located 0x7E0 bytes
/// BEFORE the metadata entry. This function scans for valid metadata entries
/// and extracts the corresponding titles.
pub fn build_song_id_title_map<R: ReadMemory>(
    reader: &R,
    text_base: u64,
    scan_size: usize,
) -> HashMap<u32, Arc<str>> {
    use encoding_rs::SHIFT_JIS;

    let metadata_base = text_base + SongInfo::METADATA_TABLE_OFFSET as u64;
    let mut result = HashMap::new();

    // Read a large chunk to scan for metadata entries
    let Ok(buffer) = reader.read_bytes(metadata_base, scan_size) else {
        warn!("Failed to read memory for song_id mapping");
        return result;
    };

    // Scan for valid (song_id, folder) pairs
    for offset in (0..buffer.len().saturating_sub(8)).step_by(4) {
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
        if result.contains_key(&(song_id as u32)) {
            continue;
        }

        // Calculate title address: metadata_addr - 0x7E0
        let metadata_addr = metadata_base + offset as u64;
        let title_addr = metadata_addr.saturating_sub(SongInfo::METADATA_TABLE_OFFSET as u64);

        // Read title (up to 64 bytes, Shift-JIS encoded)
        if let Ok(title_bytes) = reader.read_bytes(title_addr, 64) {
            let len = title_bytes.iter().position(|&b| b == 0).unwrap_or(64);
            if len > 0 {
                let (decoded, _, _) = SHIFT_JIS.decode(&title_bytes[..len]);
                let title = decoded.trim();
                if !title.is_empty() && title.chars().next().map(|c| c.is_ascii_graphic() || !c.is_ascii()).unwrap_or(false) {
                    debug!(
                        "Mapped song_id={} to title={:?} (folder={})",
                        song_id, title, folder
                    );
                    result.insert(song_id as u32, Arc::from(title));
                }
            }
        }
    }

    info!("Built song_id->title mapping with {} entries", result.len());
    result
}

/// Fetch entire song database from memory
pub fn fetch_song_database<R: ReadMemory>(
    reader: &R,
    song_list_addr: u64,
) -> Result<HashMap<u32, SongInfo>> {
    fetch_song_database_with_fixes(reader, song_list_addr, None)
}

/// Fetch entire song database from memory with optional encoding fixes
pub fn fetch_song_database_with_fixes<R: ReadMemory>(
    reader: &R,
    song_list_addr: u64,
    encoding_fixes: Option<&EncodingFixes>,
) -> Result<HashMap<u32, SongInfo>> {
    let mut result = HashMap::new();
    let mut entry_index: u64 = 0;
    let mut consecutive_failures = 0;
    const MAX_CONSECUTIVE_FAILURES: u32 = 10;

    loop {
        let address = song_list_addr + entry_index * SongInfo::MEMORY_SIZE as u64;

        // Use fallback method for new INFINITAS versions where metadata is split
        match SongInfo::read_from_memory_with_fallback(reader, address, song_list_addr, entry_index)? {
            Some(mut song) if !song.title.is_empty() && song.id > 0 => {
                // Apply encoding fixes if provided
                if let Some(fixes) = encoding_fixes {
                    if fixes.has_fix(&song.title) {
                        song.title = fixes.apply(&song.title).into();
                    }
                    if fixes.has_fix(&song.artist) {
                        song.artist = fixes.apply(&song.artist).into();
                    }
                }

                // Avoid duplicates
                if !result.contains_key(&song.id) {
                    result.insert(song.id, song);
                }
                consecutive_failures = 0;
            }
            _ => {
                consecutive_failures += 1;
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    // End of song list after multiple consecutive failures
                    debug!(
                        "Stopping song fetch after {} consecutive failures at entry {}",
                        consecutive_failures, entry_index
                    );
                    break;
                }
            }
        }

        entry_index += 1;

        // Safety limit
        if entry_index > 5000 {
            warn!("Song database fetch reached safety limit of 5000 entries");
            break;
        }
    }

    info!("Fetched {} songs from database", result.len());
    Ok(result)
}

/// Load song database from a TSV file (tracker export format)
///
/// The TSV file should have columns:
/// Title, Type, Label, Cost Normal, Cost Hyper, Cost Another, SP DJ Points, DP DJ Points,
/// SPB Unlocked, SPB Rating, ..., DPL DJ Points
///
/// This function extracts:
/// - Title (column 0)
/// - Difficulty levels (SPB Rating, SPN Rating, ... columns)
/// - Note counts (SPB Note Count, SPN Note Count, ... columns)
pub fn load_song_database_from_tsv<P: AsRef<Path>>(
    path: P,
) -> std::result::Result<HashMap<Arc<str>, SongInfo>, std::io::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut result = HashMap::new();

    // Column indices (0-based):
    // 0: Title, 1: Type, 2: Label
    // SPB: 9=Rating, 14=Note Count
    // SPN: 17=Rating, 22=Note Count
    // SPH: 25=Rating, 30=Note Count
    // SPA: 33=Rating, 38=Note Count
    // SPL: 41=Rating, 46=Note Count
    // DPN: 49=Rating, 54=Note Count (note: no DPB in this format)
    // DPH: 57=Rating, 62=Note Count
    // DPA: 65=Rating, 70=Note Count
    // DPL: 73=Rating, 78=Note Count

    const RATING_COLS: [usize; 10] = [9, 17, 25, 33, 41, 0, 49, 57, 65, 73]; // 0 for DPB (not in file)
    const NOTE_COLS: [usize; 10] = [14, 22, 30, 38, 46, 0, 54, 62, 70, 78]; // 0 for DPB

    let mut line_num = 0;
    for line_result in reader.lines() {
        line_num += 1;
        let line = line_result?;

        // Skip header
        if line_num == 1 {
            continue;
        }

        let cols: Vec<&str> = line.split('\t').collect();
        if cols.is_empty() {
            continue;
        }

        let title = cols[0].trim();
        if title.is_empty() {
            continue;
        }

        // Parse difficulty levels
        let mut levels = [0u8; 10];
        for (i, &col_idx) in RATING_COLS.iter().enumerate() {
            if col_idx > 0 && col_idx < cols.len() {
                levels[i] = cols[col_idx].parse().unwrap_or(0);
            }
        }

        // Parse note counts
        let mut total_notes = [0u32; 10];
        for (i, &col_idx) in NOTE_COLS.iter().enumerate() {
            if col_idx > 0 && col_idx < cols.len() {
                total_notes[i] = cols[col_idx].parse().unwrap_or(0);
            }
        }

        let song = SongInfo {
            id: 0, // Will be filled in when matched with memory data
            title: Arc::from(title),
            title_english: Arc::from(""),
            artist: Arc::from(""),
            genre: Arc::from(""),
            bpm: Arc::from(""),
            folder: 0,
            levels,
            total_notes,
            unlock_type: UnlockType::default(),
        };

        result.insert(Arc::from(title), song);
    }

    info!("Loaded {} songs from TSV file", result.len());
    Ok(result)
}

/// Merge memory-based song_id->title map with TSV-based title->SongInfo
///
/// This creates a complete song database by:
/// 1. Using song_id from memory scan
/// 2. Looking up song details from TSV by title
pub fn merge_song_databases(
    id_to_title: &HashMap<u32, Arc<str>>,
    tsv_db: &HashMap<Arc<str>, SongInfo>,
) -> HashMap<u32, SongInfo> {
    let mut result = HashMap::new();

    for (&song_id, title) in id_to_title {
        if let Some(tsv_song) = tsv_db.get(title) {
            let mut song = tsv_song.clone();
            song.id = song_id;
            result.insert(song_id, song);
        } else {
            // Song not in TSV, create minimal entry
            debug!("Song {} ({}) not found in TSV database", song_id, title);
            result.insert(
                song_id,
                SongInfo {
                    id: song_id,
                    title: title.clone(),
                    ..Default::default()
                },
            );
        }
    }

    info!(
        "Merged song database: {} songs (from {} memory mappings, {} TSV entries)",
        result.len(),
        id_to_title.len(),
        tsv_db.len()
    );
    result
}

/// Build song database with TSV as primary source
///
/// Strategy:
/// 1. Load TSV for complete song metadata (1749+ songs)
/// 2. Scan memory for song_id -> title mappings
/// 3. Match TSV entries to song_ids by title
/// 4. For unmatched TSV entries, create placeholder entries
///
/// This ensures we have complete song data even with lazy-loaded versions.
pub fn build_song_database_from_tsv_with_memory<R: ReadMemory>(
    reader: &R,
    song_list_addr: u64,
    tsv_path: &str,
    scan_size: usize,
) -> HashMap<u32, SongInfo> {
    use std::path::Path;

    // Step 1: Load TSV database
    let tsv_db = if Path::new(tsv_path).exists() {
        match load_song_database_from_tsv(tsv_path) {
            Ok(db) => {
                info!("Loaded {} songs from TSV", db.len());
                db
            }
            Err(e) => {
                warn!("Failed to load TSV: {}", e);
                HashMap::new()
            }
        }
    } else {
        debug!("TSV file not found: {}", tsv_path);
        HashMap::new()
    };

    // Step 2: Scan memory for song_id -> title mappings
    let memory_songs = fetch_song_database_from_memory_scan(reader, song_list_addr, scan_size);
    info!("Found {} songs in memory scan", memory_songs.len());

    // Build reverse mapping: normalized_title -> song_id
    let mut title_to_id: HashMap<String, u32> = HashMap::new();
    for song in memory_songs.values() {
        let normalized = normalize_title_for_matching(&song.title);
        title_to_id.insert(normalized, song.id);
    }

    // Step 3: Match TSV entries with song_ids
    let mut result: HashMap<u32, SongInfo> = HashMap::new();
    let mut matched_count = 0usize;
    let mut unmatched_titles: Vec<String> = Vec::new();

    for (title, tsv_song) in &tsv_db {
        let normalized = normalize_title_for_matching(title);

        if let Some(&song_id) = title_to_id.get(&normalized) {
            // Found a match - use TSV data with memory-derived song_id
            let memory_song = memory_songs.get(&song_id);
            let mut song = tsv_song.clone();
            song.id = song_id;

            // Use memory data for folder if available
            if let Some(mem) = memory_song {
                song.folder = mem.folder;
                // Prefer memory levels if available
                if mem.levels.iter().any(|&l| l > 0) {
                    song.levels = mem.levels;
                }
            }

            result.insert(song_id, song);
            matched_count += 1;
        } else {
            // No match found - track for logging
            unmatched_titles.push(title.to_string());
        }
    }

    // Step 4: Add memory-only songs (not in TSV)
    for (song_id, song) in &memory_songs {
        if !result.contains_key(song_id) {
            result.insert(*song_id, song.clone());
        }
    }

    info!(
        "Song database built: {} total ({} matched with TSV, {} TSV-only, {} memory-only)",
        result.len(),
        matched_count,
        unmatched_titles.len(),
        memory_songs.len().saturating_sub(matched_count)
    );

    if !unmatched_titles.is_empty() && unmatched_titles.len() <= 20 {
        debug!("Unmatched TSV titles: {:?}", unmatched_titles);
    } else if !unmatched_titles.is_empty() {
        debug!(
            "Unmatched TSV titles: {} (showing first 10: {:?})",
            unmatched_titles.len(),
            &unmatched_titles[..10.min(unmatched_titles.len())]
        );
    }

    result
}

/// Normalize a title for matching
///
/// Removes whitespace, converts to lowercase, and removes certain punctuation
/// to improve matching between memory and TSV titles.
fn normalize_title_for_matching(title: &str) -> String {
    title
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .filter(|c| c.is_alphanumeric() || *c > '\u{007F}') // Keep non-ASCII (Japanese)
        .collect()
}

/// Fetch a single song by its song_id from memory
///
/// This function searches through the metadata table to find a specific song.
/// Useful for dynamically loading songs that weren't found during initial scan.
pub fn fetch_song_by_id<R: ReadMemory>(
    reader: &R,
    song_list_addr: u64,
    target_song_id: u32,
    scan_size: usize,
) -> Option<SongInfo> {
    use encoding_rs::SHIFT_JIS;

    if song_list_addr == 0 {
        return None;
    }

    let metadata_base = song_list_addr + SongInfo::METADATA_TABLE_OFFSET as u64;

    // Read metadata area
    let buffer = reader.read_bytes(metadata_base, scan_size).ok()?;

    // Scan for the target song_id
    for offset in (0..buffer.len().saturating_sub(32)).step_by(4) {
        let song_id = i32::from_le_bytes([
            buffer[offset],
            buffer[offset + 1],
            buffer[offset + 2],
            buffer[offset + 3],
        ]);

        if song_id as u32 != target_song_id {
            continue;
        }

        let folder = i32::from_le_bytes([
            buffer[offset + 4],
            buffer[offset + 5],
            buffer[offset + 6],
            buffer[offset + 7],
        ]);

        // Validate folder
        if folder < 1 || folder > 50 {
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
                if !title.is_empty() {
                    Arc::from(title)
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

        debug!("Dynamically loaded song_id={} title={:?} folder={}", song_id, title, folder);

        return Some(SongInfo {
            id: song_id as u32,
            title,
            title_english: Arc::from(""),
            artist: Arc::from(""),
            genre: Arc::from(""),
            bpm: Arc::from(""),
            folder,
            levels,
            total_notes: [0; 10],
            unlock_type: UnlockType::default(),
        });
    }

    None
}

/// Build song database directly from memory for new INFINITAS versions
///
/// This function scans memory for (song_id, folder) pairs and reads corresponding
/// titles from the text table. Unlike the old approach, this works with lazy-loaded
/// data structures.
pub fn fetch_song_database_from_memory_scan<R: ReadMemory>(
    reader: &R,
    text_base: u64,
    scan_size: usize,
) -> HashMap<u32, SongInfo> {
    use encoding_rs::SHIFT_JIS;

    let metadata_base = text_base + SongInfo::METADATA_TABLE_OFFSET as u64;
    let mut result = HashMap::new();

    // Read a large chunk to scan for metadata entries
    let Ok(buffer) = reader.read_bytes(metadata_base, scan_size) else {
        warn!("Failed to read memory for song database scan");
        return result;
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
        if result.contains_key(&(song_id as u32)) {
            continue;
        }

        // Calculate title address: metadata_addr - 0x7E0
        let metadata_addr = metadata_base + offset as u64;
        let title_addr = metadata_addr.saturating_sub(SongInfo::METADATA_TABLE_OFFSET as u64);

        // Read title (up to 64 bytes, Shift-JIS encoded)
        let title = if let Ok(title_bytes) = reader.read_bytes(title_addr, 64) {
            let len = title_bytes.iter().position(|&b| b == 0).unwrap_or(64);
            if len > 0 {
                let (decoded, _, _) = SHIFT_JIS.decode(&title_bytes[..len]);
                let title = decoded.trim();
                if !title.is_empty() && title.chars().next().map(|c| c.is_ascii_graphic() || !c.is_ascii()).unwrap_or(false) {
                    Arc::from(title)
                } else {
                    continue;
                }
            } else {
                continue;
            }
        } else {
            continue;
        };

        // Try to read additional metadata (difficulty levels are ASCII at offset 8)
        let mut levels = [0u8; 10];
        if offset + 18 <= buffer.len() {
            // ASCII difficulty levels: "0111002220" format
            for (i, &byte) in buffer[offset + 8..offset + 18].iter().enumerate() {
                if byte >= b'0' && byte <= b'9' {
                    levels[i] = byte - b'0';
                }
            }
        }

        debug!("Found song_id={} title={:?} folder={}", song_id, title, folder);

        let song = SongInfo {
            id: song_id as u32,
            title,
            title_english: Arc::from(""),
            artist: Arc::from(""),
            genre: Arc::from(""),
            bpm: Arc::from(""),
            folder,
            levels,
            total_notes: [0; 10], // Not available in metadata
            unlock_type: UnlockType::default(),
        };

        result.insert(song_id as u32, song);
    }

    info!("Fetched {} songs from memory scan", result.len());
    result
}
