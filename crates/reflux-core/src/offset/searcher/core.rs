//! Core offset searcher structure and basic methods

use tracing::{debug, info};

use crate::error::{Error, Result};
use crate::offset::{OffsetSignatureSet, OffsetsCollection};
use crate::process::ReadMemory;

use super::constants::*;
use super::validation::{validate_basic_memory_access, validate_signature_offsets};

/// Core offset searcher for INFINITAS memory
pub struct OffsetSearcher<'a, R: ReadMemory> {
    pub(crate) reader: &'a R,
    pub(crate) buffer: Vec<u8>,
    pub(crate) buffer_base: u64,
}

impl<'a, R: ReadMemory> OffsetSearcher<'a, R> {
    /// Create a new offset searcher
    pub fn new(reader: &'a R) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            buffer_base: 0,
        }
    }

    /// Get the underlying reader
    pub fn reader(&self) -> &R {
        self.reader
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
        debug!("Phase 1: Searching SongList via pattern search...");
        let base = self.reader.base_address();
        let song_list_hint = base + EXPECTED_SONG_LIST_OFFSET;
        offsets.song_list = self.search_song_list_offset(song_list_hint)?;
        debug!("  SongList: 0x{:X}", offsets.song_list);

        // Phase 2: JudgeData (relative search from SongList)
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

    /// Find all matches of a pattern in the current buffer
    pub fn find_all_matches(&self, pattern: &[u8]) -> Vec<u64> {
        self.buffer
            .windows(pattern.len())
            .enumerate()
            .filter(|(_, window)| *window == pattern)
            .map(|(pos, _)| self.buffer_base + pos as u64)
            .collect()
    }

    /// Load buffer around a center address for searching
    pub fn load_buffer_around(&mut self, center: u64, distance: usize) -> Result<()> {
        let base = self.reader.base_address();
        // Don't go below base address (unmapped memory region)
        let start = center.saturating_sub(distance as u64).max(base);
        self.buffer_base = start;
        self.buffer = self.reader.read_bytes(start, distance * 2)?;
        Ok(())
    }
}
