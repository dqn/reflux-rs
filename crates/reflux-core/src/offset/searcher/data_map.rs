//! DataMap and UnlockData offset search functionality

use tracing::{debug, warn};

use crate::error::{Error, Result};
use crate::process::{ByteBuffer, ReadMemory};

use super::OffsetSearcher;
use super::constants::*;
use super::utils::merge_byte_representations;
use super::validation::OffsetValidation;

/// Probe result for DataMap candidate validation
///
/// Some fields are used only for Debug output or future enhancements.
#[derive(Debug, Clone)]
pub(crate) struct DataMapProbe {
    pub addr: u64,
    #[allow(dead_code)]
    pub table_start: u64,
    #[allow(dead_code)]
    pub table_end: u64,
    pub table_size: usize,
    #[allow(dead_code)]
    pub scanned_entries: usize,
    pub non_null_entries: usize,
    pub valid_nodes: usize,
}

impl DataMapProbe {
    pub fn is_better_than(&self, other: &Self) -> bool {
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

impl<'a, R: ReadMemory> OffsetSearcher<'a, R> {
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

    /// Probe a DataMap candidate address for validity
    pub(crate) fn probe_data_map_candidate(&self, addr: u64) -> Option<DataMapProbe> {
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
}
