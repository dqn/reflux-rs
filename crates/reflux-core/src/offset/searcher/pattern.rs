//! Pattern search utilities for memory scanning
//!
//! Provides functions for finding byte patterns in memory buffers,
//! including support for wildcard matching.

use tracing::debug;

use crate::error::{Error, Result};
use crate::process::ReadMemory;
use crate::offset::CodeSignature;

use super::constants::*;
use super::types::SearchResult;

/// Pattern search methods for OffsetSearcher
pub struct PatternSearcher<'a, R: ReadMemory> {
    reader: &'a R,
    buffer: Vec<u8>,
    buffer_base: u64,
}

impl<'a, R: ReadMemory> PatternSearcher<'a, R> {
    pub fn new(reader: &'a R) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            buffer_base: 0,
        }
    }

    /// Load a buffer around a central address
    pub fn load_buffer_around(&mut self, center: u64, distance: usize) -> Result<()> {
        let base = self.reader.base_address();
        // Don't go below base address (unmapped memory region)
        let start = center.saturating_sub(distance as u64).max(base);
        self.buffer_base = start;
        self.buffer = self.reader.read_bytes(start, distance * 2)?;
        Ok(())
    }

    /// Get the current buffer base address
    pub fn buffer_base(&self) -> u64 {
        self.buffer_base
    }

    /// Get a reference to the current buffer
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// Find the first match of a pattern in the current buffer
    pub fn find_pattern(&self, pattern: &[u8], ignore_address: Option<u64>) -> Option<usize> {
        self.buffer
            .windows(pattern.len())
            .enumerate()
            .find(|(pos, window)| {
                let addr = self.buffer_base + *pos as u64;
                *window == pattern && (ignore_address != Some(addr))
            })
            .map(|(pos, _)| pos)
    }

    /// Find all matches of a pattern in the current buffer
    pub fn find_all_matches(&self, pattern: &[u8]) -> Vec<u64> {
        self.buffer
            .windows(pattern.len())
            .enumerate()
            .filter(|(_, window)| *window == pattern)
            .map(|(pos, _)| self.buffer_base + pos as u64)
            .collect()
    }

    /// Find matches with wildcard support
    pub fn find_matches_with_wildcards(
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

    /// Search for a pattern with progressive buffer expansion
    pub fn fetch_and_search(
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

    /// Search for a pattern, returning the LAST match
    ///
    /// This avoids false positives from earlier memory regions.
    pub fn fetch_and_search_last(
        &mut self,
        hint: u64,
        pattern: &[u8],
        offset_from_match: i64,
    ) -> Result<u64> {
        let mut search_size = INITIAL_SEARCH_SIZE;
        let mut last_matches: Vec<u64> = Vec::new();

        while search_size <= MAX_SEARCH_SIZE {
            match self.load_buffer_around(hint, search_size) {
                Ok(()) => {
                    last_matches = self.find_all_matches(pattern);
                }
                Err(_) => {
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

        let last_match = *last_matches.last().expect("matches is non-empty");
        let address = last_match.wrapping_add_signed(offset_from_match);
        Ok(address)
    }

    /// Search with alternating patterns, returning the first match
    pub fn fetch_and_search_alternating(
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

    /// Scan code section for a byte pattern with wildcards
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

    /// Resolve signature targets from code references
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

            let disp =
                i32::from_le_bytes([disp_bytes[0], disp_bytes[1], disp_bytes[2], disp_bytes[3]]);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::MockMemoryBuilder;

    #[test]
    fn test_find_all_matches() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x200)
            .write_bytes(0x10, &[0xAB, 0xCD])
            .write_bytes(0x30, &[0xAB, 0xCD])
            .write_bytes(0x50, &[0xAB, 0xCD])
            .build();

        let mut searcher = PatternSearcher::new(&reader);
        searcher.load_buffer_around(0x1080, 0x80).unwrap();

        let matches = searcher.find_all_matches(&[0xAB, 0xCD]);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_find_pattern() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_bytes(0x20, &[0xDE, 0xAD, 0xBE, 0xEF])
            .build();

        let mut searcher = PatternSearcher::new(&reader);
        searcher.load_buffer_around(0x1050, 0x50).unwrap();

        let pos = searcher.find_pattern(&[0xDE, 0xAD, 0xBE, 0xEF], None);
        assert!(pos.is_some());
        assert_eq!(searcher.buffer_base() + pos.unwrap() as u64, 0x1020);
    }

    #[test]
    fn test_find_matches_with_wildcards() {
        let buffer = vec![0xAB, 0x01, 0xCD, 0xAB, 0x02, 0xCD, 0xAB, 0x03, 0xCD];
        let reader = MockMemoryBuilder::new().build();
        let searcher = PatternSearcher::new(&reader);

        // Pattern with wildcard: AB ?? CD
        let pattern = vec![Some(0xAB), None, Some(0xCD)];
        let matches = searcher.find_matches_with_wildcards(&buffer, 0x1000, &pattern);

        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0], 0x1000);
        assert_eq!(matches[1], 0x1003);
        assert_eq!(matches[2], 0x1006);
    }
}
