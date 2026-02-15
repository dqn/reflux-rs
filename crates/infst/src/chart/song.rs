use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::Result;
use crate::play::UnlockType;
use crate::process::{ByteBuffer, ReadMemory, decode_shift_jis};

use super::encoding_fixes::{fix_artist_encoding, fix_title_encoding};

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
    /// Size of one song entry in memory
    /// Version 2026012800+: 0x4B0 = 1200 bytes (was 0x3F0 = 1008 bytes in older versions)
    pub const MEMORY_SIZE: usize = 0x4B0; // 1200 bytes

    /// Offset from text table to metadata table (legacy, kept for compatibility)
    pub const METADATA_TABLE_OFFSET: usize = 0x7E0;

    // Memory layout constants
    // INFINITAS stores song metadata in fixed-size blocks with the following layout:
    const SLAB: usize = 64; // String block size (64 bytes per Shift-JIS string field)
    const WORD: usize = 4; // i32/u32 size

    // Memory offsets (relative to song entry start)
    // Version 2026012800+ layout - 3 additional 64-byte fields compared to older versions
    //
    // String fields (each 64 bytes, Shift-JIS encoded):
    //   0x000: Title
    //   0x040: Title (English)
    //   0x080: Genre
    //   0x0C0: Artist
    //   0x100-0x1BF: Additional fields (unknown purpose)
    const TITLE_OFFSET: usize = 0;
    const TITLE_ENGLISH_OFFSET: usize = Self::SLAB; // 64
    const GENRE_OFFSET: usize = Self::SLAB * 2; // 128
    const ARTIST_OFFSET: usize = Self::SLAB * 3; // 192

    // Metadata section (updated for version 2026012800+):
    // Old offsets were: folder=280, levels=288, bpm=320, notes=432, song_id=624
    // New offsets add 192 bytes (3 x 64-byte fields) to most positions
    const FOLDER_OFFSET: usize = 472; // folder byte (estimated)
    const LEVELS_OFFSET: usize = 480; // 10 bytes for difficulty levels
    const BPM_OFFSET: usize = 512; // 8 bytes: max, min (estimated)
    const NOTES_OFFSET: usize = 624; // 40 bytes: 10 x i32 (estimated)
    const SONG_ID_OFFSET: usize = 816; // 4 bytes

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
        let buf = ByteBuffer::new(&buffer);

        // Check if entry is valid (first 4 bytes should not be 0)
        if buf.read_i32_at(0).unwrap_or(0) == 0 {
            return Ok(None);
        }

        // Parse strings (Shift-JIS encoded, with encoding fixes for non-Shift-JIS characters)
        let mut title = decode_shift_jis(buf.slice_at(Self::TITLE_OFFSET, Self::SLAB)?);
        let title_english = decode_shift_jis(buf.slice_at(Self::TITLE_ENGLISH_OFFSET, Self::SLAB)?);
        let genre = decode_shift_jis(buf.slice_at(Self::GENRE_OFFSET, Self::SLAB)?);
        let mut artist = decode_shift_jis(buf.slice_at(Self::ARTIST_OFFSET, Self::SLAB)?);

        if let Some(fixed) = fix_title_encoding(&title) {
            title = fixed;
        }
        if let Some(fixed) = fix_artist_encoding(&artist) {
            artist = fixed;
        }

        // Parse folder (1 byte)
        let folder = buffer[Self::FOLDER_OFFSET] as i32;

        // Parse difficulty levels (10 bytes)
        let mut levels = [0u8; 10];
        levels.copy_from_slice(buf.slice_at(Self::LEVELS_OFFSET, 10)?);

        // Parse BPM (8 bytes: max, min)
        let bpm_max = buf.read_i32_at(Self::BPM_OFFSET)?;
        let bpm_min = buf.read_i32_at(Self::BPM_OFFSET + Self::WORD)?;

        let bpm: Arc<str> = if bpm_min != 0 && bpm_min != bpm_max {
            format!("{:03}~{:03}", bpm_min, bpm_max).into()
        } else {
            format!("{:03}", bpm_max).into()
        };

        // Parse note counts (40 bytes = 10 x i32)
        let mut total_notes = [0u32; 10];
        for (i, note_count) in total_notes.iter_mut().enumerate() {
            *note_count = buf.read_u32_at(Self::NOTES_OFFSET + i * Self::WORD)?;
        }

        // Parse song ID (4 bytes)
        let song_id = buf.read_i32_at(Self::SONG_ID_OFFSET)?;

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
                let metadata_addr = text_base
                    + Self::METADATA_TABLE_OFFSET as u64
                    + entry_index * Self::MEMORY_SIZE as u64;

                if let Ok(metadata) = reader.read_bytes(metadata_addr, 32) {
                    let buf = ByteBuffer::new(&metadata);
                    let alt_song_id = buf.read_i32_at(0).unwrap_or(0);
                    let alt_folder = buf.read_i32_at(4).unwrap_or(0);

                    // Validate: song_id should be 1000-50000, folder 1-50
                    if (1000..=50000).contains(&alt_song_id) {
                        debug!(
                            "Using metadata table for song '{}': id={}, folder={}",
                            song.title, alt_song_id, alt_folder
                        );
                        song.id = alt_song_id as u32;
                        if (1..=50).contains(&alt_folder) {
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

    let buf = ByteBuffer::new(&buffer);

    // Scan for valid song_ids (pattern: 1000-50000 followed by reasonable folder 1-50)
    let mut found_ids: Vec<(usize, i32, i32)> = Vec::new();

    for offset in (0..buffer.len() - 8).step_by(4) {
        let song_id = buf.read_i32_at(offset).unwrap_or(0);
        let folder = buf.read_i32_at(offset + 4).unwrap_or(0);

        if (1000..=50000).contains(&song_id) && (1..=50).contains(&folder) {
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
        let mut delta_counts: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        for d in &deltas {
            *delta_counts.entry(*d).or_insert(0) += 1;
        }
        if let Some((most_common, count)) = delta_counts.iter().max_by_key(|(_, v)| *v) {
            info!(
                "Most common entry size: 0x{:X} ({} bytes), {} occurrences",
                most_common, most_common, count
            );
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
        if let Ok(entry_bytes) = buf.slice_at(*offset, 32) {
            debug!("    Bytes: {:02X?}", entry_bytes);
        }
    }
}

/// Build a song_id to title mapping by scanning metadata table
///
/// For new INFINITAS versions (2026012800+), the title is located 0x7E0 bytes
/// BEFORE the metadata entry. This function scans for valid metadata entries
/// and extracts the corresponding titles.
///
/// Memory structure:
/// - text_entry[i] = text_base + i * ENTRY_SIZE
/// - meta_entry[i] = text_base + METADATA_OFFSET + i * ENTRY_SIZE
pub fn build_song_id_title_map<R: ReadMemory>(
    reader: &R,
    text_base: u64,
    scan_size: usize,
) -> HashMap<u32, Arc<str>> {
    const ENTRY_SIZE: u64 = SongInfo::MEMORY_SIZE as u64; // 0x3F0 = 1008 bytes
    const METADATA_OFFSET: u64 = SongInfo::METADATA_TABLE_OFFSET as u64; // 0x7E0 = 2016 bytes

    let mut result = HashMap::new();
    let max_entries = (scan_size as u64 / ENTRY_SIZE).min(5000);

    // Note: With lazy loading, songs may be scattered across the entry table.
    // We scan all entries without early termination to find all loaded songs.
    for i in 0..max_entries {
        let text_addr = text_base + i * ENTRY_SIZE;
        let meta_addr = text_addr + METADATA_OFFSET;

        // Read metadata
        let Ok(meta_bytes) = reader.read_bytes(meta_addr, 8) else {
            continue;
        };

        let buf = ByteBuffer::new(&meta_bytes);
        let song_id = buf.read_i32_at(0).unwrap_or(0);
        let folder = buf.read_i32_at(4).unwrap_or(0);

        // Validate song_id and folder ranges
        // Note: folder values vary widely in new INFINITAS versions (e.g., 1-200+)
        if !(1000..=90000).contains(&song_id) || !(1..=200).contains(&folder) {
            continue;
        }

        // Skip if we already have this song_id
        if result.contains_key(&(song_id as u32)) {
            continue;
        }

        // Read title from text table
        if let Ok(title_bytes) = reader.read_bytes(text_addr, 64) {
            let mut title_arc = decode_shift_jis(&title_bytes);
            if let Some(fixed) = fix_title_encoding(&title_arc) {
                title_arc = fixed;
            }
            let title = title_arc.trim();
            if !title.is_empty()
                && title
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_graphic() || !c.is_ascii())
            {
                debug!(
                    "Mapped song_id={} to title={:?} (folder={})",
                    song_id, title, folder
                );
                result.insert(song_id as u32, Arc::from(title));
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
    let mut result = HashMap::new();
    let mut entry_index: u64 = 0;
    let mut consecutive_failures = 0;
    const MAX_CONSECUTIVE_FAILURES: u32 = 10;

    loop {
        let address = song_list_addr + entry_index * SongInfo::MEMORY_SIZE as u64;

        // Use fallback method for new INFINITAS versions where metadata is split
        match SongInfo::read_from_memory_with_fallback(
            reader,
            address,
            song_list_addr,
            entry_index,
        )? {
            Some(song) if !song.title.is_empty() && song.id > 0 => {
                // Avoid duplicates
                result.entry(song.id).or_insert(song);
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
        let title = fix_title_encoding(title)
            .map(|arc| arc.to_string())
            .unwrap_or_else(|| title.to_string());
        let title = title.as_str();

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
/// This function searches through the song list entries to find a specific song.
/// song_id is stored at offset 816 within each 1200-byte entry.
///
/// Memory structure:
/// - entry[i] = song_list_addr + i * ENTRY_SIZE (0x3F0 = 1008 bytes)
/// - song_id is at offset 624 within each entry
pub fn fetch_song_by_id<R: ReadMemory>(
    reader: &R,
    song_list_addr: u64,
    target_song_id: u32,
    scan_size: usize,
) -> Option<SongInfo> {
    if song_list_addr == 0 {
        return None;
    }

    const ENTRY_SIZE: u64 = SongInfo::MEMORY_SIZE as u64; // 0x3F0 = 1008 bytes

    let max_entries = (scan_size as u64 / ENTRY_SIZE).min(5000);

    // Scan each entry for the target song_id
    for i in 0..max_entries {
        let entry_addr = song_list_addr + i * ENTRY_SIZE;

        // Use the proper read_from_memory function that reads song_id from offset 624
        match SongInfo::read_from_memory(reader, entry_addr) {
            Ok(Some(song)) if song.id == target_song_id => {
                debug!(
                    "Dynamically loaded song_id={} title={:?} folder={}",
                    song.id, song.title, song.folder
                );
                return Some(song);
            }
            _ => continue,
        }
    }

    None
}

/// Build song database directly from memory for new INFINITAS versions
///
/// This function scans memory to find all loaded songs. Each entry is 1008 bytes
/// and contains all song metadata including song_id at offset 624.
///
/// Memory structure:
/// - entry[i] = song_list_base + i * ENTRY_SIZE (0x3F0 = 1008 bytes)
pub fn fetch_song_database_from_memory_scan<R: ReadMemory>(
    reader: &R,
    song_list_base: u64,
    scan_size: usize,
) -> HashMap<u32, SongInfo> {
    const ENTRY_SIZE: u64 = SongInfo::MEMORY_SIZE as u64; // 0x3F0 = 1008 bytes

    let mut result = HashMap::new();
    let max_entries = (scan_size as u64 / ENTRY_SIZE).min(5000);

    // Note: With lazy loading, songs may be scattered across the entry table.
    // We scan all entries to find all loaded songs.
    for i in 0..max_entries {
        let entry_addr = song_list_base + i * ENTRY_SIZE;

        // Use the proper read_from_memory function
        let song = match SongInfo::read_from_memory(reader, entry_addr) {
            Ok(Some(song)) => song,
            _ => continue,
        };

        // Validate song_id range
        if song.id < 1000 || song.id > 90000 {
            continue;
        }

        // Skip if we already have this song_id
        if result.contains_key(&song.id) {
            continue;
        }

        debug!(
            "Found song_id={} title={:?} folder={}",
            song.id, song.title, song.folder
        );

        result.insert(song.id, song);
    }

    info!("Fetched {} songs from memory scan", result.len());
    result
}
