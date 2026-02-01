//! Legacy and experimental offset search methods
//!
//! This module contains methods that are currently unused but kept for
//! potential future use with new INFINITAS versions.

use tracing::debug;

use crate::error::{Error, Result};
use crate::offset::{CodeSignature, OffsetSignatureSet};
use crate::process::{ByteBuffer, ReadMemory};

use super::OffsetSearcher;
use super::constants::{
    CODE_SCAN_CHUNK_SIZE, CODE_SCAN_LIMIT, MIN_EXPECTED_SONGS, MIN_VALID_DATA_ADDRESS,
};
use super::validation::OffsetValidation;

impl<'a, R: ReadMemory> OffsetSearcher<'a, R> {
    /// Count songs using alternate structure (song_id + folder + ASCII data)
    ///
    /// Structure seems to be variable size, search for consecutive valid song_ids.
    /// Kept for potential future use with new INFINITAS versions.
    pub fn count_songs_alt_structure(&self, start_addr: u64) -> usize {
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

    /// Search for song list using code signatures (AOB scan)
    ///
    /// NOTE: This method is currently unused because signature search requires
    /// 128MB code scan and existing signatures don't work on Version 2 (2026012800+).
    /// Pattern search ("5.1.1." version string) is used instead.
    /// Kept for potential future use when stable signatures are discovered.
    pub fn search_song_list_by_signature(
        &mut self,
        signatures: &OffsetSignatureSet,
    ) -> Result<u64> {
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
        debug!("SongList signature search did not find valid candidates. Using pattern search...");
        let base = self.reader.base_address();
        self.search_song_list_offset(base)
    }

    /// Search for an offset using code signatures (AOB scan)
    ///
    /// NOTE: This method is currently unused because existing signatures don't work
    /// on newer game versions (2026012800+). Kept for potential future use when
    /// stable signatures are discovered.
    pub fn search_offset_by_signature<F>(
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

    /// Resolve signature to target addresses
    pub fn resolve_signature_targets(&self, signature: &CodeSignature) -> Result<Vec<u64>> {
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

    /// Scan code section for a pattern with wildcards
    pub fn scan_code_for_pattern(&self, pattern: &[Option<u8>]) -> Result<Vec<u64>> {
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

    /// Find all matches of a pattern with wildcards in a buffer
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

    /// Search for code that references a specific data address
    ///
    /// Looks for x64 RIP-relative LEA/MOV instructions.
    pub fn find_code_reference(&self, target_addr: u64) -> bool {
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
}
