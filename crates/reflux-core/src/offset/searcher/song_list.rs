//! SongList offset search functionality

use tracing::{debug, info, warn};

use crate::chart::SongInfo;
use crate::error::{Error, Result};
use crate::process::{ByteBuffer, ReadMemory, decode_shift_jis_to_string};

use super::OffsetSearcher;
use super::constants::*;
use super::utils::merge_byte_representations;
use super::validation::{OffsetValidation, validate_new_version_text_table};

impl<'a, R: ReadMemory> OffsetSearcher<'a, R> {
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
        info!("Trying song_id=1001 pattern search as fallback...");
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
    pub(crate) fn search_song_list_by_song_id(&mut self, base_hint: u64) -> Result<u64> {
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
}
