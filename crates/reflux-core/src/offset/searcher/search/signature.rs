//! Signature-based offset search (AOB scan)
//!
//! NOTE: This module is currently unused because existing signatures don't work
//! on newer game versions (2026012800+). Kept for potential future use when
//! stable signatures are discovered.

use tracing::debug;

use crate::error::{Error, Result};
use crate::process::{ByteBuffer, ReadMemory};
use crate::offset::searcher::validation::OffsetValidation;
use crate::offset::{CodeSignature, OffsetSignatureSet};

use super::super::constants::{
    CODE_SCAN_CHUNK_SIZE, CODE_SCAN_LIMIT, MIN_EXPECTED_SONGS, MIN_VALID_DATA_ADDRESS,
};

/// Signature search functionality for OffsetSearcher
pub trait SignatureSearch<R: ReadMemory> {
    /// Get the reader reference
    fn reader(&self) -> &R;

    /// Search for song list by signature
    ///
    /// NOTE: Currently unused - signature search doesn't work on Version 2.
    fn search_song_list_by_signature(&mut self, signatures: &OffsetSignatureSet) -> Result<u64>;

    /// Search for an offset using code signatures (AOB scan)
    ///
    /// NOTE: Currently unused - signature search doesn't work on Version 2.
    fn search_offset_by_signature<F>(
        &self,
        signatures: &OffsetSignatureSet,
        name: &str,
        validate: F,
    ) -> Result<u64>
    where
        F: Fn(u64) -> bool;

    /// Resolve signature to target addresses
    fn resolve_signature_targets(&self, signature: &CodeSignature) -> Result<Vec<u64>>;

    /// Scan code section for a pattern
    fn scan_code_for_pattern(&self, pattern: &[Option<u8>]) -> Result<Vec<u64>>;

    /// Find matches with wildcards in buffer
    fn find_matches_with_wildcards(
        buffer: &[u8],
        base_addr: u64,
        pattern: &[Option<u8>],
    ) -> Vec<u64>;
}

/// Resolve signature targets from code references
pub fn resolve_signature_targets<R: ReadMemory>(
    reader: &R,
    signature: &CodeSignature,
) -> Result<Vec<u64>> {
    let pattern = signature.pattern_bytes()?;
    let matches = scan_code_for_pattern(reader, &pattern)?;
    let mut targets = Vec::new();

    for match_addr in matches {
        let instr_addr = match_addr + signature.instr_offset as u64;
        let disp_addr = instr_addr + signature.disp_offset as u64;

        let disp_bytes = match reader.read_bytes(disp_addr, 4) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };

        let disp = ByteBuffer::new(&disp_bytes).read_i32_at(0).unwrap_or(0);
        let next_ip = instr_addr + signature.instr_len as u64;
        let mut target = next_ip.wrapping_add_signed(disp as i64);

        if signature.deref {
            match reader.read_u64(target) {
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
pub fn scan_code_for_pattern<R: ReadMemory>(
    reader: &R,
    pattern: &[Option<u8>],
) -> Result<Vec<u64>> {
    let base = reader.base_address();
    let mut results: Vec<u64> = Vec::new();
    let mut offset: u64 = 0;
    let mut scanned: usize = 0;
    let mut tail: Vec<u8> = Vec::new();

    while scanned < CODE_SCAN_LIMIT {
        let remaining = CODE_SCAN_LIMIT - scanned;
        let read_size = remaining.min(CODE_SCAN_CHUNK_SIZE);
        let addr = base + offset;

        let chunk = match reader.read_bytes(addr, read_size) {
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
        results.extend(find_matches_with_wildcards(&data, data_base, pattern));

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
pub fn find_matches_with_wildcards(
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

/// Search for song list offset using signature
///
/// NOTE: Currently unused because signature search doesn't work on Version 2.
#[allow(dead_code)]
pub fn search_song_list_by_signature<R: ReadMemory>(
    reader: &R,
    signatures: &OffsetSignatureSet,
) -> Result<u64> {
    let entry = signatures.entry("songList").ok_or_else(|| {
        Error::offset_search_failed("Signature entry 'songList' not found".to_string())
    })?;

    for signature in &entry.signatures {
        let candidates = resolve_signature_targets(reader, signature)?;
        let mut best: Option<(u64, usize)> = None;

        for addr in candidates {
            if !addr.is_multiple_of(4) {
                continue;
            }
            let song_count = reader.count_songs_at_address(addr);
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

    Err(Error::offset_search_failed(
        "SongList not found via signature search".to_string(),
    ))
}

/// Search for an offset using code signatures (AOB scan)
///
/// NOTE: Currently unused because signature search doesn't work on Version 2.
#[allow(dead_code)]
pub fn search_offset_by_signature<R, F>(
    reader: &R,
    signatures: &OffsetSignatureSet,
    name: &str,
    validate: F,
) -> Result<u64>
where
    R: ReadMemory,
    F: Fn(u64) -> bool,
{
    let entry = signatures.entry(name).ok_or_else(|| {
        Error::offset_search_failed(format!("Signature entry '{}' not found", name))
    })?;

    for signature in &entry.signatures {
        let candidates = resolve_signature_targets(reader, signature)?;
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
            .filter(|addr| validate(*addr))
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
