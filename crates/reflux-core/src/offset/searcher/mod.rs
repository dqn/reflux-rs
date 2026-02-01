//! Offset searcher for INFINITAS memory
//!
//! This module provides functionality to locate game data structures in memory.
//! It uses a combination of signature scanning and relative offset calculations.
//!
//! ## Submodules
//!
//! - `validation`: Offset validation functions
//! - `pattern`: Pattern search utilities
//! - `relative`: Relative offset search utilities

mod constants;
pub mod pattern;
pub mod relative;
pub mod search;
mod types;
mod utils;
pub mod validation;

use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::chart::SongInfo;
use crate::play::PlayType;
use crate::process::{ByteBuffer, ReadMemory, decode_shift_jis_to_string};
use crate::offset::{CodeSignature, OffsetSignatureSet, OffsetsCollection};

// Re-export validation functions and trait
pub use validation::{
    OffsetValidation, validate_basic_memory_access, validate_new_version_text_table,
    validate_signature_offsets,
};

use constants::*;
pub use types::*;
pub use utils::merge_byte_representations;

/// Probe result for DataMap candidate validation
///
/// Some fields are used only for Debug output or future enhancements.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct DataMapProbe {
    addr: u64,
    table_start: u64,
    table_end: u64,
    table_size: usize,
    scanned_entries: usize,
    non_null_entries: usize,
    valid_nodes: usize,
}

impl DataMapProbe {
    fn is_better_than(&self, other: &Self) -> bool {
        (
            self.valid_nodes,
            self.non_null_entries,
            usize::MAX - self.table_size,
        ) > (
            other.valid_nodes,
            other.non_null_entries,
            usize::MAX - other.table_size,
        )
    }
}

pub struct OffsetSearcher<'a, R: ReadMemory> {
    reader: &'a R,
    buffer: Vec<u8>,
    buffer_base: u64,
}

impl<'a, R: ReadMemory> OffsetSearcher<'a, R> {
    pub fn new(reader: &'a R) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            buffer_base: 0,
        }
    }

    /// Search for all offsets using code signatures (AOB scan)
    ///
    /// This method relies on RIP-relative code references instead of data patterns,
    /// making it more resilient to data layout changes.
    pub fn search_all_with_signatures(
        &mut self,
        signatures: &OffsetSignatureSet,
    ) -> Result<OffsetsCollection> {
        debug!("Starting signature-based offset detection...");
        let version = if signatures.version.trim().is_empty() {
            "unknown".to_string()
        } else {
            signatures.version.clone()
        };
        let mut offsets = OffsetsCollection {
            version,
            ..Default::default()
        };

        // Phase 1: SongList (anchor)
        // NOTE: Signature search is disabled because it requires 128MB code scan
        // and existing signatures don't work on Version 2 (2026012800+).
        // Pattern search ("5.1.1." version string) is reliable and much faster.
        // Start search near the expected location for faster detection.
        debug!("Phase 1: Searching SongList via pattern search...");
        let base = self.reader.base_address();
        let song_list_hint = base + EXPECTED_SONG_LIST_OFFSET;
        offsets.song_list = self.search_song_list_offset(song_list_hint)?;
        debug!("  SongList: 0x{:X}", offsets.song_list);

        // Phase 2: JudgeData (relative search from SongList)
        // NOTE: Signature search is disabled because existing signatures don't work
        // on Version 2 (2026012800+). Relative offset search is reliable and stable.
        info!("Phase 2: Searching JudgeData via relative offset from SongList...");
        offsets.judge_data = self.search_judge_data_near_song_list(offsets.song_list)?;
        info!("  JudgeData: 0x{:X}", offsets.judge_data);

        // Phase 3: PlaySettings (relative search from JudgeData)
        info!("Phase 3: Searching PlaySettings via relative offset from JudgeData...");
        offsets.play_settings = self.search_play_settings_near_judge_data(offsets.judge_data)?;
        info!("  PlaySettings: 0x{:X}", offsets.play_settings);

        // Phase 4: PlayData (relative search from PlaySettings)
        info!("Phase 4: Searching PlayData via relative offset from PlaySettings...");
        offsets.play_data = self.search_play_data_near_play_settings(offsets.play_settings)?;
        info!("  PlayData: 0x{:X}", offsets.play_data);

        // Phase 5: CurrentSong (relative search from JudgeData)
        info!("Phase 5: Searching CurrentSong via relative offset from JudgeData...");
        offsets.current_song = self.search_current_song_near_judge_data(offsets.judge_data)?;
        info!("  CurrentSong: 0x{:X}", offsets.current_song);

        // Phase 6: DataMap / UnlockData (pattern search, using SongList as hint)
        debug!("Phase 6: Searching remaining offsets with patterns...");
        let base = self.reader.base_address();
        offsets.data_map = self.search_data_map_offset(base).or_else(|e| {
            debug!(
                "  DataMap search from base failed: {}, trying from SongList",
                e
            );
            self.search_data_map_offset(offsets.song_list)
        })?;
        debug!("  DataMap: 0x{:X}", offsets.data_map);

        offsets.unlock_data = self.search_unlock_data_offset(offsets.song_list)?;
        debug!("  UnlockData: 0x{:X}", offsets.unlock_data);

        if !offsets.is_valid() {
            return Err(Error::offset_search_failed(
                "Validation failed: some offsets are zero".to_string(),
            ));
        }

        debug!("Signature-based offset detection completed successfully");
        Ok(offsets)
    }

    /// Validate all offsets in a collection (delegates to validation module)
    #[inline]
    pub fn validate_signature_offsets(&self, offsets: &OffsetsCollection) -> bool {
        validate_signature_offsets(self.reader, offsets)
    }

    /// Validate basic memory access for file-loaded offsets (delegates to validation module)
    #[inline]
    pub fn validate_basic_memory_access(&self, offsets: &OffsetsCollection) -> bool {
        validate_basic_memory_access(self.reader, offsets)
    }

    /// Search for song list offset using version string pattern
    ///
    /// Finds all matches and selects the one with the most valid songs.
    /// Also searches nearby offsets since "5.1.1." may be a header before actual song data.
    ///
    /// For new INFINITAS versions (2026012800+), the text table may only have a few entries
    /// populated due to lazy loading. In this case, we validate by checking the metadata
    /// table at text_base + 0x7E0.
    pub fn search_song_list_offset(&mut self, base_hint: u64) -> Result<u64> {
        // Pattern: "5.1.1." (version string marker)
        let pattern = b"5.1.1.";
        let mut search_size = INITIAL_SEARCH_SIZE;
        let mut best: Option<(u64, usize)> = None;
        let mut new_version_candidate: Option<u64> = None;
        let mut all_candidates: Vec<(u64, usize)> = Vec::new();

        while search_size <= MAX_SEARCH_SIZE {
            if self.load_buffer_around(base_hint, search_size).is_err() {
                break;
            }

            let matches = self.find_all_matches(pattern);
            debug!(
                "  SongList pattern search: found {} matches at search_size={}MB",
                matches.len(),
                search_size / 1024 / 1024
            );

            for addr in matches {
                if !addr.is_multiple_of(4) {
                    continue;
                }

                // Try the address itself and nearby offsets
                // "5.1.1." might be a header, with actual song list starting after it
                let offsets_to_try: &[i64] = &[
                    0,                                // Direct match
                    SongInfo::MEMORY_SIZE as i64,     // One entry after (0x3F0)
                    SongInfo::MEMORY_SIZE as i64 * 2, // Two entries after
                    -(SongInfo::MEMORY_SIZE as i64),  // One entry before
                ];

                for &offset in offsets_to_try {
                    let candidate_addr = addr.wrapping_add_signed(offset);
                    if !candidate_addr.is_multiple_of(4) {
                        continue;
                    }

                    let song_count = self.reader.count_songs_at_address(candidate_addr);
                    if offset == 0 || song_count > 1 {
                        debug!(
                            "    Candidate 0x{:X} (offset {:+}): {} songs",
                            candidate_addr, offset, song_count
                        );
                    }
                    all_candidates.push((candidate_addr, song_count));

                    // Check for new version structure (song_id in metadata table)
                    // If direct match and at least 1 song with valid title exists
                    if offset == 0
                        && song_count >= 1
                        && new_version_candidate.is_none()
                        && validate_new_version_text_table(self.reader, candidate_addr)
                    {
                        info!(
                            "  New version text table detected at 0x{:X} ({} title entries)",
                            candidate_addr, song_count
                        );
                        new_version_candidate = Some(candidate_addr);
                    }

                    if song_count < MIN_EXPECTED_SONGS {
                        continue;
                    }

                    let is_better = match best {
                        Some((_, best_count)) => song_count > best_count,
                        None => true,
                    };

                    if is_better {
                        best = Some((candidate_addr, song_count));
                    }
                }
            }

            // If we found a valid candidate with enough songs, return it
            if best.is_some() {
                break;
            }

            search_size *= 2;
        }

        // Prefer old-style match with high song count
        if let Some((addr, count)) = best {
            debug!(
                "  SongList: selected 0x{:X} ({} songs, pattern search)",
                addr, count
            );
            return Ok(addr);
        }

        // For new version: use text table if metadata table validation passed
        if let Some(addr) = new_version_candidate {
            info!(
                "  SongList: using new version text table at 0x{:X} (metadata table validated)",
                addr
            );
            return Ok(addr);
        }

        // Log all candidates for debugging
        if !all_candidates.is_empty() {
            // Sort by song count descending
            all_candidates.sort_by(|a, b| b.1.cmp(&a.1));
            warn!(
                "  SongList pattern search: no valid candidate found. Best candidates: {:?}",
                all_candidates.iter().take(5).collect::<Vec<_>>()
            );
        }

        // Fallback: search for song_id=1001 pattern (first IIDX song)
        warn!("Trying song_id=1001 pattern search as fallback...");
        if let Ok(addr) = self.search_song_list_by_song_id(base_hint) {
            return Ok(addr);
        }

        Err(Error::offset_search_failed(
            "SongList not found via pattern search".to_string(),
        ))
    }

    /// Search for song list by looking for song_id patterns (new structure)
    ///
    /// New version uses 312-byte structures with pointers to title strings.
    fn search_song_list_by_song_id(&mut self, base_hint: u64) -> Result<u64> {
        const NEW_STRUCT_SIZE: u64 = 312; // 0x138

        let search_size = 32 * 1024 * 1024; // 32MB

        if self.load_buffer_around(base_hint, search_size).is_err() {
            return Err(Error::offset_search_failed(
                "Failed to load buffer for song_id search".to_string(),
            ));
        }

        // Find song_id=1001 and song_id=1002 to locate new structure
        let pattern_1001 = merge_byte_representations(&[1001i32]);
        let pattern_1002 = merge_byte_representations(&[1002i32]);

        let matches_1001 = self.find_all_matches(&pattern_1001);
        let matches_1002 = self.find_all_matches(&pattern_1002);

        debug!(
            "  song_id search: found {} matches for 1001, {} matches for 1002",
            matches_1001.len(),
            matches_1002.len()
        );

        // Find pair with delta=312 (new structure size)
        for addr_1001 in &matches_1001 {
            for addr_1002 in &matches_1002 {
                if *addr_1002 > *addr_1001 {
                    let delta = addr_1002 - addr_1001;
                    if delta == NEW_STRUCT_SIZE {
                        debug!(
                            "  Found new structure: song_id=1001 at 0x{:X}, delta={}",
                            addr_1001, delta
                        );

                        // Dump full structure for analysis
                        if let Ok(bytes) =
                            self.reader.read_bytes(*addr_1001, NEW_STRUCT_SIZE as usize)
                        {
                            let struct_buf = ByteBuffer::new(&bytes);
                            debug!("    Full structure dump (312 bytes):");
                            debug!("      Bytes 0-31:   {:02X?}", &bytes[0..32]);
                            debug!("      Bytes 32-63:  {:02X?}", &bytes[32..64]);
                            debug!("      Bytes 64-95:  {:02X?}", &bytes[64..96]);
                            debug!("      Bytes 96-127: {:02X?}", &bytes[96..128]);

                            // Try different pointer offsets
                            for ptr_offset in [8usize, 12, 16, 20, 24, 28, 32] {
                                if ptr_offset + 8 <= bytes.len() {
                                    let ptr = struct_buf.read_u64_at(ptr_offset).unwrap_or(0);
                                    if ptr > 0x140000000
                                        && ptr < 0x150000000
                                        && let Ok(str_bytes) = self.reader.read_bytes(ptr, 32)
                                    {
                                        let s = decode_shift_jis_to_string(&str_bytes);
                                        if !s.is_empty() {
                                            debug!(
                                                "      Ptr at offset {}: 0x{:X} -> {:?}",
                                                ptr_offset, ptr, s
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Count songs to verify
                        let song_count = self.count_songs_new_structure(*addr_1001);
                        debug!("    Song count (new structure): {}", song_count);

                        if song_count >= MIN_EXPECTED_SONGS {
                            info!(
                                "  SongList (new structure): 0x{:X} ({} songs)",
                                addr_1001, song_count
                            );
                            return Ok(*addr_1001);
                        }
                    }
                }
            }
        }

        // Try alternative: SongList at 0x143186D80 with old 0x3F0 size but new layout
        // New layout: song_id at offset 0 (instead of offset 624)
        warn!("Trying new layout search (song_id at start)...");

        // Search for song_id=1001 followed by folder=43
        let alt_pattern = merge_byte_representations(&[1001i32, 43i32]);
        let alt_matches = self.find_all_matches(&alt_pattern);

        debug!("  Alt pattern search: found {} matches", alt_matches.len());

        for addr in &alt_matches {
            debug!("    Found potential new layout at 0x{:X}", addr);

            // Try counting with old structure size (0x3F0) but song_id at offset 0
            let count = self.count_songs_new_layout(*addr);
            debug!("      Song count (new layout, 0x3F0 size): {}", count);

            if count >= MIN_EXPECTED_SONGS {
                info!("  SongList (new layout): 0x{:X} ({} songs)", addr, count);
                return Ok(*addr);
            }

            // Also dump first few entries
            if count > 0 {
                for i in 0..3.min(count) {
                    let entry_addr = *addr + (i as u64 * 0x3F0);
                    if let Ok(id) = self.reader.read_i32(entry_addr) {
                        debug!("        Entry {}: song_id={} at 0x{:X}", i, id, entry_addr);
                    }
                }
            }
        }

        Err(Error::offset_search_failed(
            "New structure SongList not found".to_string(),
        ))
    }

    /// Count songs using new 312-byte structure
    fn count_songs_new_structure(&self, start_addr: u64) -> usize {
        const NEW_STRUCT_SIZE: u64 = 312;
        let mut count = 0;
        let mut addr = start_addr;

        while count < 5000 {
            // Read song_id (first 4 bytes)
            let song_id = match self.reader.read_i32(addr) {
                Ok(id) => id,
                Err(_) => break,
            };

            // Valid song IDs are >= 1000
            if !(1000..=50000).contains(&song_id) {
                break;
            }

            if count < 5 {
                debug!("      Entry {}: song_id={} at 0x{:X}", count, song_id, addr);
            }

            count += 1;
            addr += NEW_STRUCT_SIZE;
        }

        count
    }

    /// Count songs using alternate structure (song_id + folder + ASCII data)
    ///
    /// Structure seems to be variable size, search for consecutive valid song_ids.
    /// Kept for potential future use with new INFINITAS versions.
    #[allow(dead_code)]
    fn count_songs_alt_structure(&self, start_addr: u64) -> usize {
        // Try different structure sizes
        for struct_size in [32u64, 48, 64, 80, 96, 128] {
            let count = self.try_count_with_size(start_addr, struct_size);
            if count >= 100 {
                debug!(
                    "      Alt structure size {} works: {} songs",
                    struct_size, count
                );
                return count;
            }
        }
        0
    }

    #[allow(dead_code)]
    fn try_count_with_size(&self, start_addr: u64, struct_size: u64) -> usize {
        let mut count = 0;
        let mut addr = start_addr;
        let mut prev_id = 0i32;

        while count < 5000 {
            let song_id = match self.reader.read_i32(addr) {
                Ok(id) => id,
                Err(_) => break,
            };

            // Valid song IDs are >= 1000 and increasing or close
            if !(1000..=50000).contains(&song_id) {
                break;
            }

            // Check if song_id is reasonable (not too far from previous)
            if count > 0 && (song_id < prev_id - 100 || song_id > prev_id + 1000) {
                break;
            }

            prev_id = song_id;
            count += 1;
            addr += struct_size;
        }

        count
    }

    /// Comprehensive analysis of new 312-byte song structure
    ///
    /// Dumps detailed information about the structure to understand pointer layout
    pub fn analyze_new_structure(&self, start_addr: u64) {
        const NEW_STRUCT_SIZE: u64 = 312; // 0x138

        info!("=== New Structure Analysis at 0x{:X} ===", start_addr);

        // Read first few entries and analyze in detail
        for entry_idx in 0..5 {
            let entry_addr = start_addr + (entry_idx * NEW_STRUCT_SIZE);
            let Ok(buffer) = self.reader.read_bytes(entry_addr, NEW_STRUCT_SIZE as usize) else {
                warn!("  Entry {}: Failed to read", entry_idx);
                continue;
            };

            let buf = ByteBuffer::new(&buffer);

            // Read song_id (offset 0)
            let song_id = buf.read_i32_at(0).unwrap_or(0);
            if !(1000..=50000).contains(&song_id) {
                info!(
                    "  Entry {}: Invalid song_id={}, stopping",
                    entry_idx, song_id
                );
                break;
            }

            info!(
                "  Entry {} at 0x{:X}: song_id={}",
                entry_idx, entry_addr, song_id
            );

            // Analyze 32-bit compressed pointers (high 32 bits = 0x00000001)
            info!("    Compressed pointer analysis (32-bit + 0x100000000):");
            for ptr_offset in (0..312).step_by(4) {
                if ptr_offset + 4 > 312 {
                    break;
                }
                let ptr32 = buf.read_u32_at(ptr_offset).unwrap_or(0);

                // Check if this looks like a compressed pointer (high nibble 0x4)
                if ptr32 > 0x40000000 && ptr32 < 0x50000000 {
                    let ptr64 = (ptr32 as u64) + 0x100000000;
                    info!(
                        "      Offset {:3}: 0x{:08X} -> 0x{:016X}",
                        ptr_offset, ptr32, ptr64
                    );

                    // Try to read and decode what the pointer points to
                    if let Ok(target_bytes) = self.reader.read_bytes(ptr64, 128) {
                        // Try as Shift-JIS string
                        let s = decode_shift_jis_to_string(&target_bytes);
                        if !s.is_empty()
                            && s.len() > 1
                            && s.chars().take(10).all(|c| c.is_ascii_graphic() || c == ' ')
                        {
                            info!(
                                "        -> String: {:?}",
                                s.chars().take(60).collect::<String>()
                            );
                        }

                        // Show raw bytes (first 48)
                        info!(
                            "        -> Raw: {:02X?}",
                            &target_bytes[0..48.min(target_bytes.len())]
                        );

                        // Check for nested compressed pointer
                        let target_buf = ByteBuffer::new(&target_bytes);
                        let nested32 = target_buf.read_u32_at(0).unwrap_or(0);
                        if nested32 > 0x40000000 && nested32 < 0x50000000 {
                            let nested64 = (nested32 as u64) + 0x100000000;
                            if let Ok(nested_bytes) = self.reader.read_bytes(nested64, 64) {
                                let nested_s = decode_shift_jis_to_string(&nested_bytes);
                                info!(
                                    "          -> Nested ptr 0x{:X}: {:?}",
                                    nested64,
                                    nested_s.chars().take(40).collect::<String>()
                                );
                            }
                        }

                        // Also check for embedded song_id at target
                        let target_id = target_buf.read_i32_at(0).unwrap_or(0);
                        if (1000..=50000).contains(&target_id) {
                            info!("        -> Possible song_id at target: {}", target_id);
                        }
                    }
                }
            }

            // Also show interesting 32-bit values (potential offsets, flags, etc.)
            info!("    32-bit value analysis:");
            for i32_offset in (0..312).step_by(4) {
                if i32_offset + 4 > 312 {
                    break;
                }
                let val = buf.read_i32_at(i32_offset).unwrap_or(0);

                // Show non-zero values that might be meaningful
                if val != 0 && (val > 0 && val < 10000 || (1000..=50000).contains(&val)) {
                    info!(
                        "      Offset {:3}: {} (0x{:08X})",
                        i32_offset, val, val as u32
                    );
                }
            }

            // Dump structure in rows of 16 bytes
            info!("    Full hex dump:");
            for row in 0..(312 / 16) {
                let row_start = row * 16;
                let row_end = (row_start + 16).min(312);
                info!(
                    "      {:03X}: {:02X?}",
                    row_start,
                    &buffer[row_start..row_end]
                );
            }
            // Remaining bytes
            if 312 % 16 != 0 {
                let row_start = (312 / 16) * 16;
                info!("      {:03X}: {:02X?}", row_start, &buffer[row_start..312]);
            }
        }

        // Also search for the old-style text table nearby
        info!("=== Searching for text table (old style embedded strings) ===");
        let search_base = start_addr.saturating_sub(0x100000);
        if let Ok(buffer) = self.reader.read_bytes(search_base, 0x200000) {
            let pattern = b"5.1.1.";
            let mut found_count = 0;
            for (i, window) in buffer.windows(pattern.len()).enumerate() {
                if window == pattern {
                    let addr = search_base + i as u64;
                    info!("  Found '5.1.1.' at 0x{:X}", addr);
                    found_count += 1;
                    if found_count >= 5 {
                        info!("  ... (truncated, found more than 5 matches)");
                        break;
                    }
                }
            }
            if found_count == 0 {
                info!("  No '5.1.1.' pattern found in search range");
            }
        }
    }

    /// Search for song data in both old and new formats
    pub fn search_song_list_comprehensive(&mut self, base_hint: u64) -> Result<u64> {
        info!("=== Comprehensive Song List Search ===");

        // First, try old-style pattern search
        info!("Attempting old-style pattern search (embedded strings)...");
        if let Ok(addr) = self.search_song_list_offset(base_hint) {
            let song_count = self.reader.count_songs_at_address(addr);
            if song_count >= MIN_EXPECTED_SONGS {
                info!("Found via old-style: 0x{:X} ({} songs)", addr, song_count);
                return Ok(addr);
            }
        }

        // Try new structure search
        info!("Attempting new structure search (312-byte entries)...");
        if let Ok(addr) = self.search_song_list_by_song_id(base_hint) {
            self.analyze_new_structure(addr);
            return Ok(addr);
        }

        Err(Error::offset_search_failed(
            "No song list found via any method".to_string(),
        ))
    }

    /// Count songs with new layout: song_id at offset 0, struct size 0x3F0
    fn count_songs_new_layout(&self, start_addr: u64) -> usize {
        const STRUCT_SIZE: u64 = 0x3F0; // 1008 bytes, same as old
        let mut count = 0;
        let mut addr = start_addr;

        while count < 5000 {
            // Read song_id at offset 0 (new layout)
            let song_id = match self.reader.read_i32(addr) {
                Ok(id) => id,
                Err(_) => break,
            };

            // Valid song IDs are >= 1000
            if !(1000..=50000).contains(&song_id) {
                // Check if it's just zero (uninitialized) vs invalid
                if song_id == 0 && count > 0 {
                    // Try a few more entries in case of gaps
                    let mut found_more = false;
                    for skip in 1..10 {
                        let next_addr = addr + (skip * STRUCT_SIZE);
                        if let Ok(next_id) = self.reader.read_i32(next_addr)
                            && (1000..=50000).contains(&next_id)
                        {
                            found_more = true;
                            break;
                        }
                    }
                    if !found_more {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                count += 1;
            }

            addr += STRUCT_SIZE;
        }

        count
    }

    /// Search for unlock data offset
    ///
    /// Uses last match to avoid false positives from earlier memory regions.
    pub fn search_unlock_data_offset(&mut self, base_hint: u64) -> Result<u64> {
        // Pattern: 1000 (first song ID), 1 (type), 462 (unlocks)
        let pattern = merge_byte_representations(&[1000, 1, 462]);
        self.fetch_and_search_last(base_hint, &pattern, 0)
    }

    /// Search for data map offset
    pub fn search_data_map_offset(&mut self, base_hint: u64) -> Result<u64> {
        // Pattern: 0x7FFF, 0 (markers for hash map)
        let pattern = merge_byte_representations(&[0x7FFF, 0]);
        let mut search_size = INITIAL_SEARCH_SIZE;
        let mut best: Option<DataMapProbe> = None;
        let mut fallback: Option<u64> = None;

        while search_size <= MAX_SEARCH_SIZE {
            if self.load_buffer_around(base_hint, search_size).is_err() {
                break;
            }

            let matches = self.find_all_matches(&pattern);
            for match_addr in matches {
                let candidate = match_addr.wrapping_add_signed(-24);
                if fallback.is_none() {
                    fallback = Some(candidate);
                }

                let Some(probe) = self.probe_data_map_candidate(candidate) else {
                    continue;
                };

                let is_better = match &best {
                    None => true,
                    Some(current) => probe.is_better_than(current),
                };

                if is_better {
                    best = Some(probe);
                }
            }

            search_size *= 2;
        }

        if let Some(probe) = best {
            debug!(
                "  DataMap: selected 0x{:X} (valid_nodes={}, non_null_entries={}, table_size={})",
                probe.addr, probe.valid_nodes, probe.non_null_entries, probe.table_size
            );
            return Ok(probe.addr);
        }

        if let Some(addr) = fallback {
            warn!(
                "  DataMap validation failed; falling back to first match 0x{:X}",
                addr
            );
            return Ok(addr);
        }

        Err(Error::offset_search_failed(format!(
            "Pattern not found within +/-{} MB",
            MAX_SEARCH_SIZE / 1024 / 1024
        )))
    }

    /// Search for judge data offset (requires play data)
    pub fn search_judge_data_offset(
        &mut self,
        base_hint: u64,
        judge: &JudgeInput,
        play_type: PlayType,
    ) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        let (pattern_p1, pattern_p2) = self.build_judge_patterns(judge);

        let patterns = if play_type == PlayType::P1 {
            vec![pattern_p1, pattern_p2]
        } else {
            vec![pattern_p2, pattern_p1]
        };

        self.fetch_and_search_alternating(base_hint, &patterns, 0, None)
            .map(|r| r.address)
    }

    /// Search for play data offset (requires judge data to be found first)
    pub fn search_play_data_offset(
        &mut self,
        base_hint: u64,
        song_id: u32,
        difficulty: u32,
        ex_score: u32,
    ) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        // Pattern: song_id, difficulty, ex_score
        let pattern =
            merge_byte_representations(&[song_id as i32, difficulty as i32, ex_score as i32]);
        self.fetch_and_search(base_hint, &pattern, 0, None)
    }

    /// Search for current song offset
    pub fn search_current_song_offset(
        &mut self,
        base_hint: u64,
        song_id: u32,
        difficulty: u32,
    ) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        let pattern = merge_byte_representations(&[song_id as i32, difficulty as i32]);
        self.fetch_and_search(base_hint, &pattern, 0, None)
    }

    /// Search for play settings offset (requires specific settings to be set)
    ///
    /// Memory layout (matches C# implementation):
    /// - 0x00: style (4 bytes)
    /// - 0x04: gauge (4 bytes)
    /// - 0x08: assist (4 bytes)
    /// - 0x0C: flip (4 bytes)
    /// - 0x10: range (4 bytes)
    ///
    /// Uses full 20-byte pattern [style, gauge, assist, flip(0), range] for reliable matching.
    pub fn search_play_settings_offset(
        &mut self,
        base_hint: u64,
        style: i32,
        gauge: i32,
        assist: i32,
        range: i32,
    ) -> Result<u64> {
        // Full pattern: style, gauge, assist, flip(0), range - matches C# implementation
        let pattern = merge_byte_representations(&[style, gauge, assist, 0, range]);
        let mut search_size = INITIAL_SEARCH_SIZE;

        // Progressively expand search area, tolerating read errors
        while search_size <= MAX_SEARCH_SIZE {
            if self.load_buffer_around(base_hint, search_size).is_ok()
                && let Some(pos) = self.find_pattern(&pattern, None)
            {
                return Ok(self.buffer_base + pos as u64);
            }
            search_size *= 2;
        }

        Err(Error::offset_search_failed(format!(
            "Pattern not found within +/-{} MB",
            MAX_SEARCH_SIZE / 1024 / 1024
        )))
    }

    // Private helper methods

    fn load_buffer_around(&mut self, center: u64, distance: usize) -> Result<()> {
        let base = self.reader.base_address();
        // Don't go below base address (unmapped memory region)
        let start = center.saturating_sub(distance as u64).max(base);
        self.buffer_base = start;
        self.buffer = self.reader.read_bytes(start, distance * 2)?;
        Ok(())
    }

    fn probe_data_map_candidate(&self, addr: u64) -> Option<DataMapProbe> {
        let null_obj = self.reader.read_u64(addr.wrapping_sub(16)).ok()?;
        let table_start = self.reader.read_u64(addr).ok()?;
        let table_end = self.reader.read_u64(addr + 8).ok()?;

        if table_end <= table_start {
            return None;
        }

        let table_size = (table_end - table_start) as usize;
        if !(DATA_MAP_MIN_TABLE_BYTES..=DATA_MAP_MAX_TABLE_BYTES).contains(&table_size) {
            return None;
        }
        if !table_size.is_multiple_of(8) {
            return None;
        }

        let scan_size = table_size.min(DATA_MAP_SCAN_BYTES);
        let buffer = self.reader.read_bytes(table_start, scan_size).ok()?;

        let buf = ByteBuffer::new(&buffer);
        let mut non_null_entries = 0usize;
        let mut entry_points = Vec::new();
        let scanned_entries = buffer.len() / 8;

        for i in 0..scanned_entries {
            let addr = buf.read_u64_at(i * 8).unwrap_or(0);

            if addr != 0 && addr != null_obj && addr != DATA_MAP_SENTINEL {
                non_null_entries += 1;
                entry_points.push(addr);
            }
        }

        let mut valid_nodes = 0usize;
        for entry in entry_points.iter().take(DATA_MAP_NODE_SAMPLES) {
            if self.reader.validate_data_map_node(*entry) {
                valid_nodes += 1;
            }
        }

        Some(DataMapProbe {
            addr,
            table_start,
            table_end,
            table_size,
            scanned_entries,
            non_null_entries,
            valid_nodes,
        })
    }

    /// Search for song list using code signatures (AOB scan)
    ///
    /// NOTE: This method is currently unused because signature search requires
    /// 128MB code scan and existing signatures don't work on Version 2 (2026012800+).
    /// Pattern search ("5.1.1." version string) is used instead.
    /// Kept for potential future use when stable signatures are discovered.
    #[allow(dead_code)]
    fn search_song_list_by_signature(&mut self, signatures: &OffsetSignatureSet) -> Result<u64> {
        let entry = signatures.entry("songList").ok_or_else(|| {
            Error::offset_search_failed("Signature entry 'songList' not found".to_string())
        })?;

        for signature in &entry.signatures {
            let candidates = self.resolve_signature_targets(signature)?;
            let mut best: Option<(u64, usize)> = None;

            for addr in candidates {
                if !addr.is_multiple_of(4) {
                    continue;
                }
                let song_count = self.reader.count_songs_at_address(addr);
                if song_count < MIN_EXPECTED_SONGS {
                    continue;
                }

                let is_better = match best {
                    Some((_, best_count)) => song_count > best_count,
                    None => true,
                };

                if is_better {
                    best = Some((addr, song_count));
                }
            }

            if let Some((addr, count)) = best {
                debug!(
                    "  SongList: selected 0x{:X} ({} songs, signature: {})",
                    addr, count, signature.pattern
                );
                return Ok(addr);
            }
        }

        // Fallback to pattern-based search if signature search fails
        // NOTE: This is expected behavior - signature search often fails on new versions.
        // Pattern search ("5.1.1." version string) is the reliable primary method.
        debug!("SongList signature search did not find valid candidates. Using pattern search...");
        let base = self.reader.base_address();
        self.search_song_list_offset(base)
    }

    /// Search for an offset using code signatures (AOB scan)
    ///
    /// NOTE: This method is currently unused because existing signatures don't work
    /// on newer game versions (2026012800+). Kept for potential future use when
    /// stable signatures are discovered.
    #[allow(dead_code)]
    fn search_offset_by_signature<F>(
        &self,
        signatures: &OffsetSignatureSet,
        name: &str,
        validate: F,
    ) -> Result<u64>
    where
        F: Fn(&Self, u64) -> bool,
    {
        let entry = signatures.entry(name).ok_or_else(|| {
            Error::offset_search_failed(format!("Signature entry '{}' not found", name))
        })?;

        for signature in &entry.signatures {
            let candidates = self.resolve_signature_targets(signature)?;
            if !candidates.is_empty() {
                debug!(
                    "  {}: signature {} found {} raw candidates: {:X?}",
                    name,
                    signature.pattern,
                    candidates.len(),
                    &candidates[..candidates.len().min(5)]
                );
            }
            let mut valid: Vec<u64> = candidates
                .into_iter()
                .filter(|addr| addr.is_multiple_of(4))
                .filter(|addr| validate(self, *addr))
                .collect();

            if !valid.is_empty() {
                valid.sort_unstable();
                let selected = valid[0];
                debug!(
                    "  {}: selected 0x{:X} (signature: {}, candidates: {})",
                    name,
                    selected,
                    signature.pattern,
                    valid.len()
                );
                return Ok(selected);
            }
        }

        Err(Error::offset_search_failed(format!(
            "No valid candidates found for {} via signatures",
            name
        )))
    }

    fn search_near_expected<F>(&self, expected: u64, range: usize, validate: F) -> Option<u64>
    where
        F: Fn(&Self, u64) -> bool,
    {
        let range = range as u64;
        let step = 4u64;
        let mut delta = 0u64;

        while delta <= range {
            if delta == 0 {
                if expected.is_multiple_of(4) && validate(self, expected) {
                    return Some(expected);
                }
            } else {
                if expected >= delta {
                    let addr = expected - delta;
                    if addr.is_multiple_of(4) && validate(self, addr) {
                        return Some(addr);
                    }
                }

                let addr = expected + delta;
                if addr.is_multiple_of(4) && validate(self, addr) {
                    return Some(addr);
                }
            }

            delta += step;
        }

        None
    }

    fn search_judge_data_near_song_list(&self, song_list: u64) -> Result<u64> {
        let expected = song_list.wrapping_sub(JUDGE_TO_SONG_LIST);

        // First, try to find a candidate where both JudgeData and the inferred
        // CurrentSong position are valid. This cross-validation is more reliable.
        let result = self.search_near_expected(expected, JUDGE_DATA_SEARCH_RANGE, |this, addr| {
            if !this.reader.validate_judge_data_candidate(addr) {
                return false;
            }
            // Cross-validate: check if CurrentSong at expected relative position is valid
            let inferred_current_song = addr.wrapping_add(JUDGE_TO_CURRENT_SONG);
            this.reader
                .validate_current_song_address(inferred_current_song)
                .unwrap_or(false)
        });

        if let Some(addr) = result {
            return Ok(addr);
        }

        // Fallback: just validate JudgeData structure itself
        self.search_near_expected(expected, JUDGE_DATA_SEARCH_RANGE, |this, addr| {
            this.reader.validate_judge_data_candidate(addr)
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for judgeData near SongList".to_string(),
            )
        })
    }

    fn search_play_settings_near_judge_data(&self, judge_data: u64) -> Result<u64> {
        let expected = judge_data.wrapping_sub(JUDGE_TO_PLAY_SETTINGS);

        // First, try to find a candidate where both PlaySettings and the inferred
        // PlayData position are valid. This cross-validation is more reliable.
        let result =
            self.search_near_expected(expected, PLAY_SETTINGS_SEARCH_RANGE, |this, addr| {
                if this.reader.validate_play_settings_at(addr).is_none() {
                    return false;
                }
                // Cross-validate: check if PlayData at expected relative position is valid
                let inferred_play_data = addr.wrapping_add(PLAY_SETTINGS_TO_PLAY_DATA);
                this.reader
                    .validate_play_data_address(inferred_play_data)
                    .unwrap_or(false)
            });

        if let Some(addr) = result {
            return Ok(addr);
        }

        // Fallback: just validate PlaySettings structure itself
        self.search_near_expected(expected, PLAY_SETTINGS_SEARCH_RANGE, |this, addr| {
            this.reader.validate_play_settings_at(addr).is_some()
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for playSettings near JudgeData".to_string(),
            )
        })
    }

    fn search_play_data_near_play_settings(&self, play_settings: u64) -> Result<u64> {
        let expected = play_settings.wrapping_add(PLAY_SETTINGS_TO_PLAY_DATA);
        self.search_near_expected(expected, PLAY_DATA_SEARCH_RANGE, |this, addr| {
            this.reader
                .validate_play_data_address(addr)
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for playData near PlaySettings".to_string(),
            )
        })
    }

    fn search_current_song_near_judge_data(&self, judge_data: u64) -> Result<u64> {
        let expected = judge_data.wrapping_add(JUDGE_TO_CURRENT_SONG);
        self.search_near_expected(expected, CURRENT_SONG_SEARCH_RANGE, |this, addr| {
            this.reader
                .validate_current_song_address(addr)
                .unwrap_or(false)
        })
        .ok_or_else(|| {
            Error::offset_search_failed(
                "No valid candidates found for currentSong near JudgeData".to_string(),
            )
        })
    }

    fn resolve_signature_targets(&self, signature: &CodeSignature) -> Result<Vec<u64>> {
        let pattern = signature.pattern_bytes()?;
        let matches = self.scan_code_for_pattern(&pattern)?;
        let mut targets = Vec::new();

        for match_addr in matches {
            let instr_addr = match_addr + signature.instr_offset as u64;
            let disp_addr = instr_addr + signature.disp_offset as u64;

            let disp_bytes = match self.reader.read_bytes(disp_addr, 4) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };

            let disp = ByteBuffer::new(&disp_bytes).read_i32_at(0).unwrap_or(0);
            let next_ip = instr_addr + signature.instr_len as u64;
            let mut target = next_ip.wrapping_add_signed(disp as i64);

            if signature.deref {
                match self.reader.read_u64(target) {
                    Ok(ptr) => target = ptr,
                    Err(_) => continue,
                }
            }

            if signature.addend != 0 {
                target = target.wrapping_add_signed(signature.addend);
            }

            // Validate address is within expected range (above ImageBase)
            if target < MIN_VALID_DATA_ADDRESS {
                debug!(
                    "  Rejecting invalid address 0x{:X} (below MIN_VALID_DATA_ADDRESS 0x{:X})",
                    target, MIN_VALID_DATA_ADDRESS
                );
                continue;
            }

            if target != 0 {
                targets.push(target);
            }
        }

        targets.sort_unstable();
        targets.dedup();
        Ok(targets)
    }

    fn scan_code_for_pattern(&self, pattern: &[Option<u8>]) -> Result<Vec<u64>> {
        let base = self.reader.base_address();
        let mut results: Vec<u64> = Vec::new();
        let mut offset: u64 = 0;
        let mut scanned: usize = 0;
        let mut tail: Vec<u8> = Vec::new();

        while scanned < CODE_SCAN_LIMIT {
            let remaining = CODE_SCAN_LIMIT - scanned;
            let read_size = remaining.min(CODE_SCAN_CHUNK_SIZE);
            let addr = base + offset;

            let chunk = match self.reader.read_bytes(addr, read_size) {
                Ok(bytes) => bytes,
                Err(e) => {
                    if scanned == 0 {
                        return Err(Error::offset_search_failed(format!(
                            "Failed to read code section: {}",
                            e
                        )));
                    }
                    debug!(
                        "Code scan stopped at offset {:#x} (scanned {:#x} bytes): {}",
                        offset, scanned, e
                    );
                    break;
                }
            };

            let mut data = Vec::with_capacity(tail.len() + chunk.len());
            data.extend_from_slice(&tail);
            data.extend_from_slice(&chunk);

            let data_base = addr.saturating_sub(tail.len() as u64);
            results.extend(self.find_matches_with_wildcards(&data, data_base, pattern));

            if pattern.len() > 1 {
                let keep = pattern.len() - 1;
                if data.len() >= keep {
                    tail = data[data.len() - keep..].to_vec();
                } else {
                    tail = data;
                }
            } else {
                tail.clear();
            }

            scanned += read_size;
            offset += read_size as u64;
        }

        results.sort_unstable();
        results.dedup();
        Ok(results)
    }

    fn find_matches_with_wildcards(
        &self,
        buffer: &[u8],
        base_addr: u64,
        pattern: &[Option<u8>],
    ) -> Vec<u64> {
        if pattern.is_empty() || buffer.len() < pattern.len() {
            return Vec::new();
        }

        let mut results = Vec::new();
        let last = buffer.len() - pattern.len();

        'outer: for i in 0..=last {
            for (j, byte) in pattern.iter().enumerate() {
                if let Some(value) = byte
                    && buffer[i + j] != *value
                {
                    continue 'outer;
                }
            }
            results.push(base_addr + i as u64);
        }

        results
    }

    fn fetch_and_search(
        &mut self,
        hint: u64,
        pattern: &[u8],
        offset_from_match: i64,
        ignore_address: Option<u64>,
    ) -> Result<u64> {
        let mut search_size = INITIAL_SEARCH_SIZE;

        while search_size <= MAX_SEARCH_SIZE {
            self.load_buffer_around(hint, search_size)?;

            if let Some(pos) = self.find_pattern(pattern, ignore_address) {
                let address =
                    (self.buffer_base + pos as u64).wrapping_add_signed(offset_from_match);
                return Ok(address);
            }

            search_size *= 2;
        }

        Err(Error::offset_search_failed(format!(
            "Pattern not found within +/-{} MB",
            MAX_SEARCH_SIZE / 1024 / 1024
        )))
    }

    /// Like fetch_and_search, but returns the LAST match instead of first.
    /// Expands search area progressively and uses the last match found.
    /// This avoids false positives from earlier memory regions (e.g., 2016-build data).
    fn fetch_and_search_last(
        &mut self,
        hint: u64,
        pattern: &[u8],
        offset_from_match: i64,
    ) -> Result<u64> {
        let mut search_size = INITIAL_SEARCH_SIZE;
        let mut last_matches: Vec<u64> = Vec::new();

        // Keep expanding to find all matches across the readable memory area
        while search_size <= MAX_SEARCH_SIZE {
            match self.load_buffer_around(hint, search_size) {
                Ok(()) => {
                    last_matches = self.find_all_matches(pattern);
                }
                Err(_) => {
                    // Memory read failed, use results from previous size
                    break;
                }
            }
            search_size *= 2;
        }

        if last_matches.is_empty() {
            return Err(Error::offset_search_failed(format!(
                "Pattern not found within +/-{} MB",
                MAX_SEARCH_SIZE / 1024 / 1024
            )));
        }

        // Use last match to avoid false positives from earlier regions
        let last_match = *last_matches.last().expect("matches is non-empty");
        let address = last_match.wrapping_add_signed(offset_from_match);
        Ok(address)
    }

    fn fetch_and_search_alternating(
        &mut self,
        hint: u64,
        patterns: &[Vec<u8>],
        offset_from_match: i64,
        ignore_address: Option<u64>,
    ) -> Result<SearchResult> {
        let mut search_size = INITIAL_SEARCH_SIZE;

        while search_size <= MAX_SEARCH_SIZE {
            self.load_buffer_around(hint, search_size)?;

            for (index, pattern) in patterns.iter().enumerate() {
                if let Some(pos) = self.find_pattern(pattern, ignore_address) {
                    let address =
                        (self.buffer_base + pos as u64).wrapping_add_signed(offset_from_match);
                    return Ok(SearchResult {
                        address,
                        pattern_index: index,
                    });
                }
            }

            search_size *= 2;
        }

        Err(Error::offset_search_failed(format!(
            "None of {} patterns found within +/-{} MB",
            patterns.len(),
            MAX_SEARCH_SIZE / 1024 / 1024
        )))
    }

    fn build_judge_patterns(&self, judge: &JudgeInput) -> (Vec<u8>, Vec<u8>) {
        // P1 pattern: P1 judgments, then zeros for P2
        let pattern_p1 = merge_byte_representations(&[
            judge.pgreat as i32,
            judge.great as i32,
            judge.good as i32,
            judge.bad as i32,
            judge.poor as i32,
            0,
            0,
            0,
            0,
            0, // P2 zeros
            judge.combo_break as i32,
            0,
            judge.fast as i32,
            0,
            judge.slow as i32,
            0,
        ]);

        // P2 pattern: zeros for P1, then P2 judgments
        let pattern_p2 = merge_byte_representations(&[
            0,
            0,
            0,
            0,
            0, // P1 zeros
            judge.pgreat as i32,
            judge.great as i32,
            judge.good as i32,
            judge.bad as i32,
            judge.poor as i32,
            0,
            judge.combo_break as i32,
            0,
            judge.fast as i32,
            0,
            judge.slow as i32,
        ]);

        (pattern_p1, pattern_p2)
    }
    fn find_pattern(&self, pattern: &[u8], ignore_address: Option<u64>) -> Option<usize> {
        self.buffer
            .windows(pattern.len())
            .enumerate()
            .find(|(pos, window)| {
                let addr = self.buffer_base + *pos as u64;
                *window == pattern && (ignore_address != Some(addr))
            })
            .map(|(pos, _)| pos)
    }
}

impl<'a, R: ReadMemory> OffsetSearcher<'a, R> {
    /// Run interactive offset search with user prompts
    ///
    /// This method guides the user through the offset discovery process:
    /// 1. Search SongList, UnlockData, DataMap
    /// 2. User plays "Sleepless Days SPA" and enters judge data
    /// 3. Search JudgeData, PlayData, CurrentSong
    /// 4. User sets specific options and searches PlaySettings
    pub fn interactive_search<P: SearchPrompter>(
        &mut self,
        prompter: &P,
        old_offsets: &OffsetsCollection,
        new_version: &str,
    ) -> Result<InteractiveSearchResult> {
        prompter.prompt_continue("Starting offset search mode, press ENTER to continue");

        let mut new_offsets = OffsetsCollection {
            version: new_version.to_string(),
            ..Default::default()
        };

        // Use base address as default hint if old offsets are invalid
        let base = self.reader.base_address();
        let hint = |offset: u64| if offset == 0 { base } else { offset };

        // Phase 1: Static patterns
        prompter.display_message("Searching for SongList...");
        new_offsets.song_list = self.search_song_list_offset(hint(old_offsets.song_list))?;
        prompter.display_message(&format!("Found SongList at 0x{:X}", new_offsets.song_list));

        prompter.display_message("Searching for UnlockData...");
        new_offsets.unlock_data = self.search_unlock_data_offset(hint(old_offsets.unlock_data))?;
        prompter.display_message(&format!(
            "Found UnlockData at 0x{:X}",
            new_offsets.unlock_data
        ));

        prompter.display_message("Searching for DataMap...");
        // Use SongList as hint for DataMap since they are in similar memory region
        let data_map_hint = if old_offsets.data_map != 0 {
            old_offsets.data_map
        } else {
            new_offsets.song_list
        };
        new_offsets.data_map = self.search_data_map_offset(data_map_hint)?;
        prompter.display_message(&format!("Found DataMap at 0x{:X}", new_offsets.data_map));

        // Phase 2: Judge data (requires playing a song)
        prompter.prompt_continue(
            "Play Sleepless Days SPA, either fully or exit after hitting 50-ish notes or more, then press ENTER"
        );

        prompter.display_message("Enter your judge data:");
        let judge = JudgeInput {
            pgreat: prompter.prompt_number("Enter pgreat count: "),
            great: prompter.prompt_number("Enter great count: "),
            good: prompter.prompt_number("Enter good count: "),
            bad: prompter.prompt_number("Enter bad count: "),
            poor: prompter.prompt_number("Enter poor count: "),
            combo_break: prompter.prompt_number("Enter combobreak count: "),
            fast: prompter.prompt_number("Enter fast count: "),
            slow: prompter.prompt_number("Enter slow count: "),
        };

        // Try P1 pattern first, then P2
        prompter.display_message("Searching for JudgeData...");
        let (judge_address, play_type) =
            self.search_judge_data_with_playtype(hint(old_offsets.judge_data), &judge)?;
        new_offsets.judge_data = judge_address;
        prompter.display_message(&format!(
            "Found JudgeData at 0x{:X} ({})",
            new_offsets.judge_data,
            play_type.short_name()
        ));

        // Phase 3: Play data and current song (Sleepless Days SPA = 25094, difficulty 3)
        let ex_score = judge.pgreat * 2 + judge.great;
        prompter.display_message("Searching for PlayData...");
        new_offsets.play_data =
            self.search_play_data_offset(hint(old_offsets.play_data), 25094, 3, ex_score)?;
        prompter.display_message(&format!("Found PlayData at 0x{:X}", new_offsets.play_data));

        prompter.display_message("Searching for CurrentSong...");
        let current_song_addr =
            self.search_current_song_offset(hint(old_offsets.current_song), 25094, 3)?;
        // Verify it's different from PlayData
        new_offsets.current_song = if current_song_addr == new_offsets.play_data {
            self.search_current_song_offset_excluding(
                hint(old_offsets.current_song),
                25094,
                3,
                Some(new_offsets.play_data),
            )?
        } else {
            current_song_addr
        };
        prompter.display_message(&format!(
            "Found CurrentSong at 0x{:X}",
            new_offsets.current_song
        ));

        // Phase 4: Play settings (requires user to set specific options)
        // C# prompts: "RANDOM EXHARD OFF SUDDEN+" and "MIRROR EASY AUTO-SCRATCH HIDDEN+"
        prompter.prompt_continue(
            "Set the following settings and then press ENTER: RANDOM EXHARD OFF SUDDEN+",
        );

        prompter.display_message("Searching for PlaySettings...");
        // RANDOM=1, EXHARD=4, OFF=0, SUDDEN+=1 (C# values)
        let settings_addr1 = self.search_play_settings_offset(
            hint(old_offsets.play_settings),
            1, // RANDOM (style)
            4, // EXHARD (gauge) - C# uses 4 for EXHARD
            0, // OFF (assist)
            1, // SUDDEN+ (range)
        )?;

        prompter.prompt_continue(
            "Now set the following settings and then press ENTER: MIRROR EASY AUTO-SCRATCH HIDDEN+",
        );

        // MIRROR=4, EASY=2, AUTO-SCRATCH=1, HIDDEN+=2
        let settings_addr2 = self.search_play_settings_offset(
            hint(old_offsets.play_settings),
            4, // MIRROR (style)
            2, // EASY (gauge)
            1, // AUTO-SCRATCH (assist)
            2, // HIDDEN+ (range)
        )?;

        if settings_addr1 != settings_addr2 {
            prompter
                .display_warning("Warning: Settings addresses don't match between two searches!");
        }

        // Adjust for P2 offset if needed
        new_offsets.play_settings = if play_type == PlayType::P2 {
            use crate::play::Settings;
            settings_addr1 - Settings::P2_OFFSET
        } else {
            settings_addr1
        };
        prompter.display_message(&format!(
            "Found PlaySettings at 0x{:X}",
            new_offsets.play_settings
        ));

        prompter.display_message("Offset search complete!");

        Ok(InteractiveSearchResult {
            offsets: new_offsets,
            play_type,
        })
    }

    /// Search for judge data and determine play type
    fn search_judge_data_with_playtype(
        &mut self,
        base_hint: u64,
        judge: &JudgeInput,
    ) -> Result<(u64, PlayType)> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        let (pattern_p1, pattern_p2) = self.build_judge_patterns(judge);
        let patterns = vec![pattern_p1, pattern_p2];

        let result = self.fetch_and_search_alternating(base_hint, &patterns, 0, None)?;

        let play_type = if result.pattern_index == 0 {
            PlayType::P1
        } else {
            PlayType::P2
        };

        Ok((result.address, play_type))
    }

    /// Search for current song offset, excluding a specific address
    fn search_current_song_offset_excluding(
        &mut self,
        base_hint: u64,
        song_id: u32,
        difficulty: u32,
        exclude: Option<u64>,
    ) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        let pattern = merge_byte_representations(&[song_id as i32, difficulty as i32]);
        self.fetch_and_search(base_hint, &pattern, 0, exclude)
    }
    /// Find all matches of a pattern in the current buffer
    fn find_all_matches(&self, pattern: &[u8]) -> Vec<u64> {
        self.buffer
            .windows(pattern.len())
            .enumerate()
            .filter(|(_, window)| *window == pattern)
            .map(|(pos, _)| self.buffer_base + pos as u64)
            .collect()
    }

    // ==========================================================================
    // Code signature search (AOB scan) for validation
    // ==========================================================================

    // Validate offsets by searching for code references
    //
    // This searches for x64 RIP-relative addressing instructions (LEA, MOV)
    // that reference the found offsets. If found, it increases confidence.

    /// Search for code that references a specific data address
    ///
    /// Looks for x64 RIP-relative LEA/MOV instructions.
    #[allow(dead_code)]
    fn find_code_reference(&self, target_addr: u64) -> bool {
        // Search for LEA rcx/rdx/rax, [rip+disp32] patterns
        // 48 8D 0D xx xx xx xx  (LEA rcx, [rip+disp32])
        // 48 8D 15 xx xx xx xx  (LEA rdx, [rip+disp32])
        // 48 8D 05 xx xx xx xx  (LEA rax, [rip+disp32])
        let lea_prefixes = [
            [0x48, 0x8D, 0x0D], // LEA rcx
            [0x48, 0x8D, 0x15], // LEA rdx
            [0x48, 0x8D, 0x05], // LEA rax
        ];

        for prefix in lea_prefixes {
            for (pos, window) in self.buffer.windows(7).enumerate() {
                if window[0..3] == prefix {
                    // Extract RIP-relative offset.
                    // The slice window[3..7] is guaranteed to be exactly 4 bytes due to windows(7),
                    // so try_into() cannot fail in practice. We use explicit error handling rather
                    // than unwrap_or with a zero fallback to avoid silent failures.
                    let offset_bytes: [u8; 4] = match window[3..7].try_into() {
                        Ok(bytes) => bytes,
                        Err(_) => continue,
                    };
                    let rel_offset = i32::from_le_bytes(offset_bytes);

                    // Calculate absolute address
                    // RIP points to next instruction (current_pos + 7)
                    let code_addr = self.buffer_base + pos as u64;
                    let next_ip = code_addr + 7;
                    let ref_addr = next_ip.wrapping_add_signed(rel_offset as i64);

                    if ref_addr == target_addr {
                        return true;
                    }
                }
            }
        }

        false
    }

    // Dump current values at detected offsets for verification (compact format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::MockMemoryBuilder;
    use crate::process::layout::{judge, settings};
    use crate::offset::OffsetsCollection;

    #[test]
    fn test_validate_judge_data_candidate_valid() {
        // STATE_MARKER_1 is at offset 0xD8 (WORD * 54)
        // STATE_MARKER_2 is at offset 0xDC (WORD * 55)
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 50) // Valid marker (0-100)
            .write_i32(judge::STATE_MARKER_2 as usize, 50) // Valid marker (0-100)
            .build();

        assert!(reader.validate_judge_data_candidate(0x1000));
    }

    #[test]
    fn test_validate_judge_data_candidate_invalid_markers() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 200) // Invalid marker (> 100)
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        // Should fail because first marker is > 100
        assert!(!reader.validate_judge_data_candidate(0x1000));
    }

    #[test]
    fn test_validate_play_settings_valid() {
        // SONG_SELECT_MARKER is at WORD * 6 = 24 bytes before PlaySettings
        // So if PlaySettings is at offset 0x18, song_select_marker is at 0x18 - 0x18 = 0
        let marker_offset = settings::SONG_SELECT_MARKER as usize; // 24

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            // PlaySettings at offset 0x18 (marker_offset)
            // song_select_marker at offset 0
            .write_i32(0, 1) // song_select_marker
            .write_i32(marker_offset, 2) // style = R-RANDOM
            .write_i32(marker_offset + 4, 3) // gauge = HARD
            .write_i32(marker_offset + 8, 0) // assist = OFF
            .write_i32(marker_offset + 12, 0) // flip = OFF
            .write_i32(marker_offset + 16, 2) // range = HIDDEN+
            .build();

        let result = reader.validate_play_settings_at(0x1000 + marker_offset as u64);
        assert!(result.is_some());
    }

    #[test]
    fn test_validate_play_settings_invalid_style() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1) // song_select_marker
            .write_i32(marker_offset, 10) // style = INVALID (> 6)
            .write_i32(marker_offset + 4, 2)
            .write_i32(marker_offset + 8, 0)
            .write_i32(marker_offset + 12, 0)
            .write_i32(marker_offset + 16, 1)
            .build();

        let result = reader.validate_play_settings_at(0x1000 + marker_offset as u64);
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_play_data_valid() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500) // song_id in range
            .write_i32(4, 3) // difficulty (SPA)
            .write_i32(8, 2000) // ex_score
            .write_i32(12, 25) // miss_count
            .build();

        let result = reader.validate_play_data_address(0x1000).unwrap();
        assert!(result);
    }

    #[test]
    fn test_validate_play_data_all_zeros_is_rejected() {
        // Initial state (all zeros) should be rejected during offset search
        // to avoid false positives at wrong addresses
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 0) // song_id
            .write_i32(4, 0) // difficulty
            .write_i32(8, 0) // ex_score
            .write_i32(12, 0) // miss_count
            .build();

        let result = reader.validate_play_data_address(0x1000).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_validate_play_data_invalid_song_id() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 500) // song_id below 1000
            .write_i32(4, 3)
            .write_i32(8, 2000)
            .write_i32(12, 25)
            .build();

        let result = reader.validate_play_data_address(0x1000).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_validate_current_song_valid() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 2500) // song_id
            .write_i32(4, 5) // difficulty (DPB)
            .write_i32(8, 500) // field3
            .build();

        let result = reader.validate_current_song_address(0x1000).unwrap();
        assert!(result);
    }

    #[test]
    fn test_validate_current_song_power_of_two_rejected() {
        // Powers of 2 are likely memory artifacts
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 2048) // song_id = 2^11 (power of 2)
            .write_i32(4, 3)
            .write_i32(8, 500)
            .build();
        let result = reader.validate_current_song_address(0x1000).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_validate_data_map_valid() {
        let base = 0x1000u64;
        let table_start = base + 0x100;
        let table_end = table_start + 0x4000; // 16KB table

        let reader = MockMemoryBuilder::new()
            .base(base)
            .with_size(0x8000)
            .write_u64(0, table_start)
            .write_u64(8, table_end)
            .build();

        assert!(reader.validate_data_map_address(base));
    }

    #[test]
    fn test_validate_data_map_invalid_size() {
        let base = 0x1000u64;
        let table_start = base + 0x100;
        let table_end = table_start + 0x100; // Only 256 bytes - too small

        let reader = MockMemoryBuilder::new()
            .base(base)
            .with_size(0x1000)
            .write_u64(0, table_start)
            .write_u64(8, table_end)
            .build();

        assert!(!reader.validate_data_map_address(base));
    }

    #[test]
    fn test_validate_unlock_data_valid() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500) // song_id in range [1000, 50000]
            .write_i32(4, 2) // unlock_type = Bits (0-3 valid)
            .build();

        assert!(reader.validate_unlock_data_address(0x1000));
    }

    #[test]
    fn test_validate_unlock_data_invalid_song_id() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 500) // song_id too low
            .write_i32(4, 1)
            .build();

        assert!(!reader.validate_unlock_data_address(0x1000));
    }

    #[test]
    fn test_validate_unlock_data_invalid_type() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500)
            .write_i32(4, 10) // unlock_type out of range
            .build();

        assert!(!reader.validate_unlock_data_address(0x1000));
    }

    #[test]
    fn test_merge_byte_representations() {
        let bytes = merge_byte_representations(&[1000, 42]);
        // 1000 = 0x000003E8, 42 = 0x0000002A in little-endian
        assert_eq!(bytes.len(), 8);
        assert_eq!(&bytes[0..4], &[0xE8, 0x03, 0x00, 0x00]);
        assert_eq!(&bytes[4..8], &[0x2A, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_find_all_matches() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x200) // Large enough for the search
            .write_bytes(0x10, &[0xAB, 0xCD])
            .write_bytes(0x30, &[0xAB, 0xCD])
            .write_bytes(0x50, &[0xAB, 0xCD])
            .build();

        let mut searcher = OffsetSearcher::new(&reader);
        // Load buffer around the center with smaller distance
        searcher.load_buffer_around(0x1080, 0x80).unwrap();

        let matches = searcher.find_all_matches(&[0xAB, 0xCD]);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_offsets_collection_is_valid() {
        let valid = OffsetsCollection {
            version: "test".to_string(),
            song_list: 0x1000,
            judge_data: 0x2000,
            play_settings: 0x3000,
            play_data: 0x4000,
            current_song: 0x5000,
            data_map: 0x6000,
            unlock_data: 0x7000,
        };
        assert!(valid.is_valid());

        let invalid = OffsetsCollection {
            version: "test".to_string(),
            song_list: 0, // Zero = invalid
            judge_data: 0x2000,
            play_settings: 0x3000,
            play_data: 0x4000,
            current_song: 0x5000,
            data_map: 0x6000,
            unlock_data: 0x7000,
        };
        assert!(!invalid.is_valid());
    }
}
