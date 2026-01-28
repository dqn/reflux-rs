//! Offset searcher for INFINITAS memory

mod constants;
mod types;
mod utils;

use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::game::{PlayType, SongInfo};
use crate::memory::ReadMemory;
use crate::memory::layout::{judge, settings};
use crate::offset::{CodeSignature, OffsetSignatureSet, OffsetsCollection};

use constants::*;
pub use types::*;
use utils::is_power_of_two;
pub use utils::merge_byte_representations;

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
        debug!("Phase 1: Searching SongList via signatures...");
        offsets.song_list = self.search_song_list_by_signature(signatures)?;
        debug!("  SongList: 0x{:X}", offsets.song_list);

        // Phase 2: JudgeData
        debug!("Phase 2: Searching JudgeData via signatures...");
        offsets.judge_data =
            match self.search_offset_by_signature(signatures, "judgeData", |this, addr| {
                this.validate_judge_data_candidate(addr)
            }) {
                Ok(addr) => addr,
                Err(e) => {
                    warn!(
                        "JudgeData signature search failed: {}. Falling back to relative search...",
                        e
                    );
                    self.search_judge_data_near_song_list(offsets.song_list)?
                }
            };
        debug!("  JudgeData: 0x{:X}", offsets.judge_data);

        // Phase 3: PlaySettings
        debug!("Phase 3: Searching PlaySettings via signatures...");
        offsets.play_settings = match self.search_offset_by_signature(
            signatures,
            "playSettings",
            |this, addr| this.validate_play_settings_at(addr).is_some(),
        ) {
            Ok(addr) => addr,
            Err(e) => {
                warn!(
                    "PlaySettings signature search failed: {}. Falling back to relative search...",
                    e
                );
                self.search_play_settings_near_judge_data(offsets.judge_data)?
            }
        };
        debug!("  PlaySettings: 0x{:X}", offsets.play_settings);

        // Phase 4: PlayData
        debug!("Phase 4: Searching PlayData via signatures...");
        offsets.play_data =
            match self.search_offset_by_signature(signatures, "playData", |this, addr| {
                this.validate_play_data_address(addr).unwrap_or(false)
            }) {
                Ok(addr) => addr,
                Err(e) => {
                    info!(
                        "PlayData signature search failed: {}. Falling back to relative search...",
                        e
                    );
                    self.search_play_data_near_play_settings(offsets.play_settings)?
                }
            };
        debug!("  PlayData: 0x{:X}", offsets.play_data);

        // Phase 5: CurrentSong
        debug!("Phase 5: Searching CurrentSong via signatures...");
        offsets.current_song = match self.search_offset_by_signature(
            signatures,
            "currentSong",
            |this, addr| this.validate_current_song_address(addr).unwrap_or(false),
        ) {
            Ok(addr) => addr,
            Err(e) => {
                warn!(
                    "CurrentSong signature search failed: {}. Falling back to relative search...",
                    e
                );
                self.search_current_song_near_judge_data(offsets.judge_data)?
            }
        };
        debug!("  CurrentSong: 0x{:X}", offsets.current_song);

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
            return Err(Error::OffsetSearchFailed(
                "Validation failed: some offsets are zero".to_string(),
            ));
        }

        debug!("Signature-based offset detection completed successfully");
        Ok(offsets)
    }

    pub fn validate_signature_offsets(&self, offsets: &OffsetsCollection) -> bool {
        if offsets.song_list == 0
            || offsets.judge_data == 0
            || offsets.play_settings == 0
            || offsets.play_data == 0
            || offsets.current_song == 0
        {
            return false;
        }

        if self.count_songs_at_address(offsets.song_list) < MIN_EXPECTED_SONGS {
            return false;
        }
        if !self.validate_judge_data_candidate(offsets.judge_data) {
            return false;
        }
        if self
            .validate_play_settings_at(offsets.play_settings)
            .is_none()
        {
            return false;
        }
        if !self
            .validate_play_data_address(offsets.play_data)
            .unwrap_or(false)
        {
            return false;
        }
        if !self
            .validate_current_song_address(offsets.current_song)
            .unwrap_or(false)
        {
            return false;
        }

        let within_range = |actual: u64, expected: u64, range: u64| {
            if actual >= expected {
                actual - expected <= range
            } else {
                expected - actual <= range
            }
        };

        let judge_to_play = offsets.judge_data.wrapping_sub(offsets.play_settings);
        if !within_range(
            judge_to_play,
            JUDGE_TO_PLAY_SETTINGS,
            PLAY_SETTINGS_SEARCH_RANGE as u64,
        ) {
            return false;
        }

        let song_to_judge = offsets.song_list.wrapping_sub(offsets.judge_data);
        if !within_range(
            song_to_judge,
            JUDGE_TO_SONG_LIST,
            JUDGE_DATA_SEARCH_RANGE as u64,
        ) {
            return false;
        }

        let play_data_delta = offsets.play_data.wrapping_sub(offsets.play_settings);
        if !within_range(
            play_data_delta,
            PLAY_SETTINGS_TO_PLAY_DATA,
            PLAY_DATA_SEARCH_RANGE as u64,
        ) {
            return false;
        }

        let current_song_delta = offsets.current_song.wrapping_sub(offsets.judge_data);
        if !within_range(
            current_song_delta,
            JUDGE_TO_CURRENT_SONG,
            CURRENT_SONG_SEARCH_RANGE as u64,
        ) {
            return false;
        }

        true
    }

    /// Search for song list offset using version string pattern
    pub fn search_song_list_offset(&mut self, base_hint: u64) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        // Pattern: "5.1.1." (version string marker)
        let pattern = b"5.1.1.";
        self.fetch_and_search(base_hint, pattern, 0, None)
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

        Err(Error::OffsetSearchFailed(format!(
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
    /// Memory layout:
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
            if let Ok(()) = self.load_buffer_around(base_hint, search_size) {
                if let Some(pos) = self.find_pattern(&pattern, None) {
                    return Ok(self.buffer_base + pos as u64);
                }
            }
            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(format!(
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

        let mut non_null_entries = 0usize;
        let mut entry_points = Vec::new();
        let scanned_entries = buffer.len() / 8;

        for i in 0..scanned_entries {
            let addr = u64::from_le_bytes([
                buffer[i * 8],
                buffer[i * 8 + 1],
                buffer[i * 8 + 2],
                buffer[i * 8 + 3],
                buffer[i * 8 + 4],
                buffer[i * 8 + 5],
                buffer[i * 8 + 6],
                buffer[i * 8 + 7],
            ]);

            if addr != 0 && addr != null_obj && addr != DATA_MAP_SENTINEL {
                non_null_entries += 1;
                entry_points.push(addr);
            }
        }

        let mut valid_nodes = 0usize;
        for entry in entry_points.iter().take(DATA_MAP_NODE_SAMPLES) {
            if self.validate_data_map_node(*entry) {
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

    fn validate_data_map_node(&self, addr: u64) -> bool {
        let buffer = match self.reader.read_bytes(addr, 64) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        if buffer.len() < 52 {
            return false;
        }

        let diff = i32::from_le_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
        let song_id = i32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
        let playtype = i32::from_le_bytes([buffer[24], buffer[25], buffer[26], buffer[27]]);
        let score = u32::from_le_bytes([buffer[32], buffer[33], buffer[34], buffer[35]]);
        let miss_count = u32::from_le_bytes([buffer[36], buffer[37], buffer[38], buffer[39]]);
        let lamp = i32::from_le_bytes([buffer[48], buffer[49], buffer[50], buffer[51]]);

        if !(0..=4).contains(&diff) {
            return false;
        }
        if !(0..=1).contains(&playtype) {
            return false;
        }
        if !(MIN_SONG_ID..=MAX_SONG_ID).contains(&song_id) {
            return false;
        }
        if score > 200_000 {
            return false;
        }
        if miss_count > 10_000 && miss_count != u32::MAX {
            return false;
        }
        if !(0..=7).contains(&lamp) {
            return false;
        }

        true
    }

    fn search_song_list_by_signature(&self, signatures: &OffsetSignatureSet) -> Result<u64> {
        let entry = signatures.entry("songList").ok_or_else(|| {
            Error::OffsetSearchFailed("Signature entry 'songList' not found".to_string())
        })?;

        for signature in &entry.signatures {
            let candidates = self.resolve_signature_targets(signature)?;
            let mut best: Option<(u64, usize)> = None;

            for addr in candidates {
                if !addr.is_multiple_of(4) {
                    continue;
                }
                let song_count = self.count_songs_at_address(addr);
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

        Err(Error::OffsetSearchFailed(
            "SongList not found via signatures".to_string(),
        ))
    }

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
            Error::OffsetSearchFailed(format!("Signature entry '{}' not found", name))
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

        Err(Error::OffsetSearchFailed(format!(
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
        self.search_near_expected(expected, JUDGE_DATA_SEARCH_RANGE, |this, addr| {
            this.validate_judge_data_candidate(addr)
        })
        .ok_or_else(|| {
            Error::OffsetSearchFailed(
                "No valid candidates found for judgeData near SongList".to_string(),
            )
        })
    }

    fn search_play_settings_near_judge_data(&self, judge_data: u64) -> Result<u64> {
        let expected = judge_data.wrapping_sub(JUDGE_TO_PLAY_SETTINGS);
        self.search_near_expected(expected, PLAY_SETTINGS_SEARCH_RANGE, |this, addr| {
            this.validate_play_settings_at(addr).is_some()
        })
        .ok_or_else(|| {
            Error::OffsetSearchFailed(
                "No valid candidates found for playSettings near JudgeData".to_string(),
            )
        })
    }

    fn search_play_data_near_play_settings(&self, play_settings: u64) -> Result<u64> {
        let expected = play_settings.wrapping_add(PLAY_SETTINGS_TO_PLAY_DATA);
        self.search_near_expected(expected, PLAY_DATA_SEARCH_RANGE, |this, addr| {
            this.validate_play_data_address(addr).unwrap_or(false)
        })
        .ok_or_else(|| {
            Error::OffsetSearchFailed(
                "No valid candidates found for playData near PlaySettings".to_string(),
            )
        })
    }

    fn search_current_song_near_judge_data(&self, judge_data: u64) -> Result<u64> {
        let expected = judge_data.wrapping_add(JUDGE_TO_CURRENT_SONG);
        self.search_near_expected(expected, CURRENT_SONG_SEARCH_RANGE, |this, addr| {
            this.validate_current_song_address(addr).unwrap_or(false)
        })
        .ok_or_else(|| {
            Error::OffsetSearchFailed(
                "No valid candidates found for currentSong near JudgeData".to_string(),
            )
        })
    }

    fn validate_judge_data_candidate(&self, addr: u64) -> bool {
        if !addr.is_multiple_of(4) {
            return false;
        }

        let marker1 = self
            .reader
            .read_i32(addr + judge::STATE_MARKER_1)
            .unwrap_or(-1);
        let marker2 = self
            .reader
            .read_i32(addr + judge::STATE_MARKER_2)
            .unwrap_or(-1);

        (0..=100).contains(&marker1) && (0..=100).contains(&marker2)
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
                        return Err(Error::OffsetSearchFailed(format!(
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

        Err(Error::OffsetSearchFailed(format!(
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
            return Err(Error::OffsetSearchFailed(format!(
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

        Err(Error::OffsetSearchFailed(format!(
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

    /// Count how many songs can be read from a given song list address.
    ///
    /// This is used to validate SongList candidates by checking if they
    /// actually point to valid song data.
    fn count_songs_at_address(&self, song_list_addr: u64) -> usize {
        let mut count = 0;
        let mut current_position: u64 = 0;

        // Read up to a reasonable limit to avoid infinite loops.
        // 5000 is chosen because INFINITAS has approximately 2000+ songs as of 2025,
        // so this limit provides ample headroom for future expansion while preventing
        // runaway iteration on invalid addresses.
        const MAX_SONGS_TO_CHECK: usize = 5000;

        while count < MAX_SONGS_TO_CHECK {
            let address = song_list_addr + current_position;

            match SongInfo::read_from_memory(self.reader, address) {
                Ok(Some(song)) if !song.title.is_empty() => {
                    count += 1;
                }
                _ => {
                    // End of song list or invalid data
                    break;
                }
            }

            current_position += SongInfo::MEMORY_SIZE as u64;
        }

        count
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
        // RANDOM=1, EXHARD=5, OFF=0, SUDDEN+=1
        let settings_addr1 = self.search_play_settings_offset(
            hint(old_offsets.play_settings),
            1, // RANDOM (style)
            5, // EXHARD (gauge)
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
            use crate::game::Settings;
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

    /// Validate if the given address contains valid PlaySettings
    ///
    /// Memory layout:
    /// - 0x00: style (4 bytes)
    /// - 0x04: gauge (4 bytes)
    /// - 0x08: assist (4 bytes)
    /// - 0x0C: flip (4 bytes)
    /// - 0x10: range (4 bytes)
    fn validate_play_settings_at(&self, addr: u64) -> Option<u64> {
        let style = self.reader.read_i32(addr).ok()?;
        let gauge = self.reader.read_i32(addr + 4).ok()?;
        let assist = self.reader.read_i32(addr + 8).ok()?;
        let flip = self.reader.read_i32(addr + 12).ok()?;
        let range = self.reader.read_i32(addr + 16).ok()?;

        // Valid ranges check (aligned with C# implementation)
        // style: OFF(0), RANDOM(1), R-RANDOM(2), S-RANDOM(3), MIRROR(4),
        //        SYNCHRONIZE RANDOM(5), SYMMETRY RANDOM(6)
        // gauge: OFF(0), ASSIST EASY(1), EASY(2), NORMAL(3), HARD(4), EXHARD(5)
        // assist: OFF(0), AUTO SCRATCH(1), 5KEYS(2), LEGACY NOTE(3),
        //         KEY ASSIST(4), ANY KEY(5)
        // flip: OFF(0), ON(1)
        // range: OFF(0), SUDDEN+(1), HIDDEN+(2), SUD+ & HID+(3),
        //        LIFT(4), LIFT & SUD+(5)
        if !(0..=6).contains(&style)
            || !(0..=5).contains(&gauge)
            || !(0..=5).contains(&assist)
            || !(0..=1).contains(&flip)
            || !(0..=5).contains(&range)
        {
            return None;
        }

        // Additional validation: song_select_marker should be 0 or 1
        // This prevents false positives from addresses that happen to have
        // valid-looking settings but incorrect song_select_marker
        let song_select_marker = self
            .reader
            .read_i32(addr.wrapping_sub(settings::SONG_SELECT_MARKER))
            .ok()?;
        if !(0..=1).contains(&song_select_marker) {
            return None;
        }

        Some(addr)
    }

    /// Validate if an address contains valid PlayData
    fn validate_play_data_address(&self, addr: u64) -> Result<bool> {
        let song_id = self.reader.read_i32(addr).unwrap_or(-1);
        let difficulty = self.reader.read_i32(addr + 4).unwrap_or(-1);
        let ex_score = self.reader.read_i32(addr + 8).unwrap_or(-1);
        let miss_count = self.reader.read_i32(addr + 12).unwrap_or(-1);

        // Accept initial state (all zeros) - common when not in song select
        if song_id == 0 && difficulty == 0 && ex_score == 0 && miss_count == 0 {
            return Ok(true);
        }

        // Require song_id in valid IIDX range (>= 1000)
        let is_valid_play_data = (MIN_SONG_ID..=MAX_SONG_ID).contains(&song_id)
            && (0..=9).contains(&difficulty)
            && (0..=10000).contains(&ex_score)
            && (0..=3000).contains(&miss_count);

        Ok(is_valid_play_data)
    }

    /// Validate if an address contains valid CurrentSong data
    fn validate_current_song_address(&self, addr: u64) -> Result<bool> {
        let song_id = self.reader.read_i32(addr).unwrap_or(-1);
        let difficulty = self.reader.read_i32(addr + 4).unwrap_or(-1);

        // Accept initial state (zeros)
        if song_id == 0 && difficulty == 0 {
            return Ok(true);
        }

        // song_id must be in realistic range (IIDX song IDs start from ~1000)
        if !(1000..=50000).contains(&song_id) {
            return Ok(false);
        }
        // Filter out powers of 2 which are likely memory artifacts
        if is_power_of_two(song_id as u32) {
            return Ok(false);
        }
        if !(0..=9).contains(&difficulty) {
            return Ok(false);
        }

        // Additional validation: check that the third field is reasonable
        let field3 = self.reader.read_i32(addr + 8).unwrap_or(-1);
        if !(0..=10000).contains(&field3) {
            return Ok(false);
        }

        Ok(true)
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
