use crate::error::{Error, Result};
use crate::game::PlayType;
use crate::memory::MemoryReader;
use crate::offset::OffsetsCollection;

const INITIAL_SEARCH_SIZE: usize = 2 * 1024 * 1024; // 2MB
const MAX_SEARCH_SIZE: usize = 300 * 1024 * 1024; // 300MB

/// Judge data for interactive offset searching
#[derive(Debug, Clone, Default)]
pub struct JudgeInput {
    pub pgreat: u32,
    pub great: u32,
    pub good: u32,
    pub bad: u32,
    pub poor: u32,
    pub combo_break: u32,
    pub fast: u32,
    pub slow: u32,
}

/// Search result with address and matching pattern index
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub address: u64,
    pub pattern_index: usize,
}

pub struct OffsetSearcher<'a> {
    reader: &'a MemoryReader<'a>,
    buffer: Vec<u8>,
    buffer_base: u64,
}

impl<'a> OffsetSearcher<'a> {
    pub fn new(reader: &'a MemoryReader<'a>) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            buffer_base: 0,
        }
    }

    /// Search for all offsets automatically (non-interactive)
    pub fn search_all(&mut self) -> Result<OffsetsCollection> {
        let mut offsets = OffsetsCollection::default();

        // Load memory buffer
        self.load_buffer_around(self.reader.base_address(), INITIAL_SEARCH_SIZE)?;

        // Search for version string to find song list offset
        offsets.version = self.search_version()?;
        offsets.song_list = self.search_song_list()?;

        Ok(offsets)
    }

    /// Search for song list offset using version string pattern
    pub fn search_song_list_offset(&mut self, base_hint: u64) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        // Pattern: "5.1.1." (version string marker)
        let pattern = b"5.1.1.";
        self.fetch_and_search(base_hint, pattern, 0, None)
    }

    /// Search for unlock data offset
    pub fn search_unlock_data_offset(&mut self, base_hint: u64) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        // Pattern: 1000 (first song ID), 1 (type), 462 (unlocks)
        let pattern = merge_byte_representations(&[1000, 1, 462]);
        self.fetch_and_search(base_hint, &pattern, 0, None)
    }

    /// Search for data map offset
    pub fn search_data_map_offset(&mut self, base_hint: u64) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        // Pattern: 0x7FFF, 0 (markers for hash map)
        let pattern = merge_byte_representations(&[0x7FFF, 0]);
        // Offset back 3 steps in 8-byte address space
        self.fetch_and_search(base_hint, &pattern, -24, None)
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
        let pattern = merge_byte_representations(&[song_id as i32, difficulty as i32, ex_score as i32]);
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
    pub fn search_play_settings_offset(
        &mut self,
        base_hint: u64,
        style: i32,
        gauge: i32,
        assist: i32,
        range: i32,
    ) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        // Pattern: style, gauge, assist, 0, range
        let pattern = merge_byte_representations(&[style, gauge, assist, 0, range]);
        self.fetch_and_search(base_hint, &pattern, 0, None)
    }

    // Private helper methods

    fn load_buffer_around(&mut self, center: u64, distance: usize) -> Result<()> {
        let start = center.saturating_sub(distance as u64);
        self.buffer_base = start;
        self.buffer = self.reader.read_bytes(start, distance * 2)?;
        Ok(())
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
                let address = (self.buffer_base + pos as u64).wrapping_add_signed(offset_from_match);
                return Ok(address);
            }

            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(format!(
            "Pattern not found within {} MB",
            MAX_SEARCH_SIZE / 1024 / 1024
        )))
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
                    let address = (self.buffer_base + pos as u64).wrapping_add_signed(offset_from_match);
                    return Ok(SearchResult {
                        address,
                        pattern_index: index,
                    });
                }
            }

            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(format!(
            "None of {} patterns found within {} MB",
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
            0, 0, 0, 0, 0, // P2 zeros
            judge.combo_break as i32,
            0,
            judge.fast as i32,
            0,
            judge.slow as i32,
            0,
        ]);

        // P2 pattern: zeros for P1, then P2 judgments
        let pattern_p2 = merge_byte_representations(&[
            0, 0, 0, 0, 0, // P1 zeros
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

    fn search_version(&self) -> Result<String> {
        let pattern = b"P2D:J:B:A:";

        if let Some(pos) = self.find_pattern(pattern, None) {
            let end = self.buffer[pos..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| pos + p)
                .unwrap_or(pos + 30);

            let version_bytes = &self.buffer[pos..end.min(pos + 30)];
            let version = String::from_utf8_lossy(version_bytes).to_string();
            return Ok(version);
        }

        Err(Error::OffsetSearchFailed(
            "Version string not found".to_string(),
        ))
    }

    fn search_song_list(&self) -> Result<u64> {
        let pattern = b"P2D:J:B:A:";
        if let Some(pos) = self.find_pattern(pattern, None) {
            return Ok(self.buffer_base + pos as u64);
        }

        Err(Error::OffsetSearchFailed(
            "Song list offset not found".to_string(),
        ))
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

/// Convert i32 values to little-endian byte representation
pub fn merge_byte_representations(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_byte_representations() {
        let bytes = merge_byte_representations(&[1, 2]);
        assert_eq!(bytes.len(), 8);
        assert_eq!(bytes[0..4], [1, 0, 0, 0]);
        assert_eq!(bytes[4..8], [2, 0, 0, 0]);
    }
}
