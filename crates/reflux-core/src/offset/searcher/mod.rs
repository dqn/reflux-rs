//! Offset searcher for INFINITAS memory

mod constants;
mod types;
mod utils;

use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::game::{PlayType, SongInfo};
use crate::memory::ReadMemory;
use crate::memory::layout::judge;
use crate::offset::OffsetsCollection;

use constants::*;
pub use types::*;
use utils::is_power_of_two;
pub use utils::merge_byte_representations;

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

    /// Search for all offsets automatically (non-interactive)
    ///
    /// This method attempts to find all offsets without user interaction.
    /// Strategy: Find SongList first (most reliable), then use relative positions.
    ///
    /// Detection order:
    /// 1. SongList (anchor) - using version string and song count validation
    /// 2. JudgeData - relative to SongList with fallback
    /// 3. PlaySettings - relative to JudgeData with fallback
    /// 4. PlayData - relative to PlaySettings with fallback
    /// 5. CurrentSong - relative to JudgeData with fallback
    /// 6. DataMap, UnlockData - using existing patterns
    pub fn search_all(&mut self) -> Result<OffsetsCollection> {
        info!("Starting automatic offset detection...");
        let mut offsets = OffsetsCollection::default();
        let base = self.reader.base_address();

        // Phase 1: Search SongList (most reliable anchor point)
        // This uses song count validation which is highly reliable
        info!("Phase 1: Searching SongList (anchor)...");
        match self.search_version_and_song_list(base) {
            Ok((version, song_list)) => {
                offsets.version = version;
                offsets.song_list = song_list;
                info!("  SongList: 0x{:X}", offsets.song_list);
                info!("  Version: {}", offsets.version);
            }
            Err(e) => {
                warn!("  SongList search failed: {}", e);
                return Err(e);
            }
        }

        // Phase 2: Search JudgeData relative to SongList
        // judgeData = songList - JUDGE_TO_SONG_LIST (approximately)
        info!("Phase 2: Searching JudgeData relative to SongList...");
        offsets.judge_data = self
            .search_judge_data_near_song_list_narrow(offsets.song_list)
            .or_else(|e| {
                info!(
                    "  JudgeData narrow search failed: {}, trying flexible search",
                    e
                );
                self.search_judge_data_flexible(offsets.song_list)
            })?;
        info!("  JudgeData: 0x{:X}", offsets.judge_data);

        // Phase 3: Search relative offsets with fallback
        info!("Phase 3: Searching relative offsets...");

        // PlaySettings: narrow search → full search
        offsets.play_settings = self
            .search_play_settings_near_judge_narrow(offsets.judge_data)
            .or_else(|e| {
                info!(
                    "  PlaySettings narrow search failed: {}, trying fallback",
                    e
                );
                self.search_play_settings_by_values(offsets.judge_data)
            })?;
        info!("  PlaySettings: 0x{:X}", offsets.play_settings);

        // PlayData: narrow search → full search
        offsets.play_data = self
            .search_play_data_near_settings_narrow(offsets.play_settings)
            .or_else(|e| {
                info!("  PlayData narrow search failed: {}, trying fallback", e);
                self.search_play_data_near_settings(offsets.play_settings)
            })?;
        info!("  PlayData: 0x{:X}", offsets.play_data);

        // CurrentSong: narrow search → full search
        offsets.current_song = self
            .search_current_song_near_judge_narrow(offsets.judge_data, offsets.play_data)
            .or_else(|e| {
                info!("  CurrentSong narrow search failed: {}, trying fallback", e);
                self.search_current_song_near_judge(offsets.judge_data, offsets.play_data)
            })?;
        info!("  CurrentSong: 0x{:X}", offsets.current_song);

        // Phase 4: Search remaining offsets
        info!("Phase 4: Searching remaining offsets...");

        // DataMap: search from base → search from songList
        offsets.data_map = self.search_data_map_offset(base).or_else(|e| {
            info!(
                "  DataMap search from base failed: {}, trying from songList",
                e
            );
            self.search_data_map_offset(offsets.song_list)
        })?;
        info!("  DataMap: 0x{:X}", offsets.data_map);

        // UnlockData
        offsets.unlock_data = self.search_unlock_data_offset(offsets.song_list)?;
        info!("  UnlockData: 0x{:X}", offsets.unlock_data);

        // Phase 5: Validation
        info!("Phase 5: Validating offsets...");
        if !offsets.is_valid() {
            warn!("Offset validation failed");
            return Err(Error::OffsetSearchFailed(
                "Validation failed: some offsets are zero".to_string(),
            ));
        }

        // Phase 6: Code signature validation (optional, for increased confidence)
        info!("Phase 6: Code signature validation...");
        let signature_matches = self.validate_offsets_with_signatures(&offsets);
        if signature_matches > 0 {
            info!(
                "  Validated {} offset(s) with code signatures",
                signature_matches
            );
        } else {
            debug!("  No code signature matches found (this is OK)");
        }

        // Phase 7: Dump current values for verification
        debug!("Phase 7: Dumping current values at detected offsets...");
        self.dump_offset_values(&offsets);

        info!("Automatic offset detection completed successfully");
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
        let base = self.reader.base_address();
        // Don't go below base address (unmapped memory region)
        let start = center.saturating_sub(distance as u64).max(base);
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
                let address =
                    (self.buffer_base + pos as u64).wrapping_add_signed(offset_from_match);
                return Ok(address);
            }

            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(format!(
            "Pattern not found within {} MB",
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
                "Pattern not found within {} MB",
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

    /// Search for version string and song list
    ///
    /// This method searches for "5.1.1." pattern (first song's version) to find
    /// the SongList, then separately searches for "P2D:J:B:A:" to get the version string.
    fn search_version_and_song_list(&mut self, base_hint: u64) -> Result<(String, u64)> {
        // Search for SongList using "5.1.1." pattern (first song's version marker)
        let song_list_pattern = b"5.1.1.";
        let mut search_size = INITIAL_SEARCH_SIZE;
        let mut all_matches: Vec<u64> = Vec::new();

        // Progressively expand search area until memory read fails
        while search_size <= MAX_SEARCH_SIZE {
            match self.load_buffer_around(base_hint, search_size) {
                Ok(()) => {
                    all_matches = self.find_all_matches(song_list_pattern);
                }
                Err(_) => break,
            }
            search_size *= 2;
        }

        if all_matches.is_empty() {
            return Err(Error::OffsetSearchFailed(
                "SongList pattern (5.1.1.) not found within search area".to_string(),
            ));
        }

        // Try candidates from first to last (use the "top one" per C# implementation)
        // Validate each by counting readable songs
        let mut song_list_addr: Option<u64> = None;
        let mut selected_song_count = 0;
        for &candidate in &all_matches {
            let song_count = self.count_songs_at_address(candidate);
            if song_count >= MIN_EXPECTED_SONGS {
                song_list_addr = Some(candidate);
                selected_song_count = song_count;
                break;
            }
        }

        if let Some(addr) = song_list_addr {
            debug!(
                "  SongList: {} candidates, selected 0x{:X} ({} songs)",
                all_matches.len(),
                addr,
                selected_song_count
            );
        }

        let song_list = song_list_addr.ok_or_else(|| {
            let candidates_info: Vec<String> = all_matches
                .iter()
                .take(5)
                .map(|&addr| {
                    let count = self.count_songs_at_address(addr);
                    format!("0x{:X} ({} songs)", addr, count)
                })
                .collect();

            Error::OffsetSearchFailed(format!(
                "No SongList candidate passed validation (>= {} songs). Candidates: {}",
                MIN_EXPECTED_SONGS,
                candidates_info.join(", ")
            ))
        })?;

        // Now search for version string using "P2D:J:B:A:" pattern
        let version = self.search_version_string(base_hint)?;

        Ok((version, song_list))
    }

    /// Search for game version string using "P2D:J:B:A:" pattern
    fn search_version_string(&mut self, base_hint: u64) -> Result<String> {
        let version_pattern = b"P2D:J:B:A:";
        let mut search_size = INITIAL_SEARCH_SIZE;
        let mut all_matches: Vec<u64> = Vec::new();

        while search_size <= MAX_SEARCH_SIZE {
            match self.load_buffer_around(base_hint, search_size) {
                Ok(()) => {
                    all_matches = self.find_all_matches(version_pattern);
                }
                Err(_) => break,
            }
            search_size *= 2;
        }

        if all_matches.is_empty() {
            return Err(Error::OffsetSearchFailed(
                "Version string (P2D:J:B:A:) not found".to_string(),
            ));
        }

        // Use last match (most likely to be the active version)
        let version_addr = *all_matches.last().expect("matches is non-empty");

        // Extract version string from buffer
        let pos = (version_addr - self.buffer_base) as usize;
        let end = self.buffer[pos..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| pos + p)
            .unwrap_or(pos + 30);

        let version_bytes = &self.buffer[pos..end.min(pos + 30)];
        let version = String::from_utf8_lossy(version_bytes).to_string();

        debug!("  Found version string: {}", version);
        Ok(version)
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
        prompter.prompt_continue(
            "Set the following settings and then press ENTER: RANDOM EXHARD OFF SUDDEN+",
        );

        prompter.display_message("Searching for PlaySettings...");
        // RANDOM=1, EXHARD=4, OFF=0, SUDDEN+=1
        let settings_addr1 = self.search_play_settings_offset(
            hint(old_offsets.play_settings),
            1, // RANDOM
            4, // EXHARD
            0, // OFF
            1, // SUDDEN+
        )?;

        prompter.prompt_continue(
            "Now set the following settings and then press ENTER: MIRROR EASY AUTO-SCRATCH HIDDEN+",
        );

        // MIRROR=4, EASY=2, AUTO-SCRATCH=1, HIDDEN+=2
        let settings_addr2 = self.search_play_settings_offset(
            hint(old_offsets.play_settings),
            4, // MIRROR
            2, // EASY
            1, // AUTO-SCRATCH
            2, // HIDDEN+
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

    // ==========================================================================
    // Automatic offset detection methods (non-interactive)
    // ==========================================================================

    /// Narrow search for JudgeData near SongList using relative offset
    ///
    /// Search range: ±64KB around expected position (songList - JUDGE_TO_SONG_LIST)
    /// This is the most reliable anchor detection method.
    fn search_judge_data_near_song_list_narrow(&mut self, song_list: u64) -> Result<u64> {
        let center = song_list.saturating_sub(JUDGE_TO_SONG_LIST);
        let range = JUDGE_DATA_SEARCH_RANGE;

        if self.load_buffer_around(center, range).is_err() {
            return Err(Error::OffsetSearchFailed("Memory load failed".to_string()));
        }

        // Search for 72-byte zero pattern (initial state) or during-play pattern
        let zero_pattern = vec![0u8; judge::INITIAL_ZERO_SIZE];
        let mut candidates = self.find_all_matches(&zero_pattern);

        // Sort candidates by distance from expected center to prioritize closest matches
        candidates.sort_by_key(|&c| c.abs_diff(center));

        // Validate each candidate
        for &candidate in &candidates {
            // Check distance from expected center - should be within range
            let distance_from_center = candidate.abs_diff(center);
            if distance_from_center > range as u64 {
                continue;
            }

            // STATE_MARKER validation
            let marker1 = self
                .reader
                .read_i32(candidate + judge::STATE_MARKER_1)
                .unwrap_or(-1);
            let marker2 = self
                .reader
                .read_i32(candidate + judge::STATE_MARKER_2)
                .unwrap_or(-1);

            // Valid markers are 0 (song select) or small positive values (during/after play)
            if !(0..=100).contains(&marker1) || !(0..=100).contains(&marker2) {
                continue;
            }

            // PlaySettings validation: search for valid settings near expected position
            // The JUDGE_TO_PLAY_SETTINGS constant may have a fixed offset (~0xE8) from the actual
            // position, so we search within ±0x100 (256 bytes) tolerance to find valid settings.
            let expected_play_settings = candidate.saturating_sub(JUDGE_TO_PLAY_SETTINGS);
            let search_range = 0x100u64; // Allow ±256 bytes tolerance for offset variations

            let mut found_play_settings_addr: Option<u64> = None;
            let search_start = expected_play_settings.saturating_sub(search_range);
            let search_end = expected_play_settings.saturating_add(search_range);

            // Search in 4-byte aligned positions
            let mut pos = search_start & !3; // Align to 4 bytes
            while pos <= search_end {
                if self.validate_play_settings_at(pos).is_some() {
                    found_play_settings_addr = Some(pos);
                    break;
                }
                pos += 4;
            }

            let play_settings_addr = match found_play_settings_addr {
                Some(addr) => addr,
                None => continue,
            };

            // Chain validation: PlaySettings → PlayData
            // If PlaySettings is found, verify that PlayData at expected position is in initial state
            let expected_play_data = play_settings_addr + PLAY_SETTINGS_TO_PLAY_DATA;
            if !self.validate_play_data_initial_state(expected_play_data) {
                continue;
            }

            // CurrentSong validation: check if expected position has valid song data
            // This helps distinguish correct JudgeData from false positives
            let current_song_addr = candidate + JUDGE_TO_CURRENT_SONG;
            let current_song_id = self.reader.read_i32(current_song_addr).unwrap_or(-1);
            let current_difficulty = self.reader.read_i32(current_song_addr + 4).unwrap_or(-1);

            // Valid CurrentSong: song_id in valid range (1000-50000), difficulty 0-9
            // Note: We don't accept initial state (0, 0) because it can cause false positives
            // in the song select screen where a valid song should always be selected
            let is_valid_song = (MIN_SONG_ID..=MAX_SONG_ID).contains(&current_song_id)
                && (0..=9).contains(&current_difficulty);

            if !is_valid_song {
                continue;
            }

            debug!(
                "  JudgeData: {} candidates, selected 0x{:X} (narrow search, CurrentSong: song_id={}, diff={})",
                candidates.len(),
                candidate,
                current_song_id,
                current_difficulty
            );
            return Ok(candidate);
        }

        // If zero pattern search failed, try during-play validation
        let base = self.reader.base_address();
        let start = center.saturating_sub(range as u64).max(base);
        let end = center.saturating_add(range as u64);

        let mut candidate = (start + 3) & !3;
        while candidate < end {
            if self.validate_judge_data_during_play(candidate) {
                debug!(
                    "  JudgeData: selected 0x{:X} (during-play validation)",
                    candidate
                );
                return Ok(candidate);
            }
            candidate += 4;
        }

        Err(Error::OffsetSearchFailed(
            "JudgeData not found in narrow range from SongList".to_string(),
        ))
    }

    /// Search for JudgeData flexibly (initial state or during-play)
    ///
    /// Fallback method when narrow search fails.
    fn search_judge_data_flexible(&mut self, hint: u64) -> Result<u64> {
        // Try initial state pattern first
        if let Ok(addr) = self.search_judge_data_initial_state(hint) {
            debug!("  JudgeData: selected 0x{:X} (initial state)", addr);
            return Ok(addr);
        }

        // Fallback: during-play detection
        match self.search_judge_data_during_play(hint) {
            Ok(addr) => {
                debug!("  JudgeData: selected 0x{:X} (during-play)", addr);
                Ok(addr)
            }
            Err(e) => Err(e),
        }
    }

    /// Narrow search for PlaySettings near JudgeData using relative offset
    ///
    /// Search range: ±8KB around expected position (judgeData - 0x2ACEE8)
    fn search_play_settings_near_judge_narrow(&mut self, judge_data: u64) -> Result<u64> {
        let center = judge_data.saturating_sub(JUDGE_TO_PLAY_SETTINGS);
        let range = PLAY_SETTINGS_SEARCH_RANGE;

        // Load buffer around the expected position
        if self.load_buffer_around(center, range).is_err() {
            return Err(Error::OffsetSearchFailed("Memory load failed".to_string()));
        }

        // Search for valid PlaySettings pattern
        let base = self.reader.base_address();
        let start = center.saturating_sub(range as u64).max(base);
        let end = center.saturating_add(range as u64);

        // Collect all valid candidates
        let mut candidates: Vec<u64> = Vec::new();
        let mut candidate = (start + 3) & !3; // 4-byte alignment

        while candidate < end {
            // Bounds check
            if candidate < self.buffer_base
                || candidate + 20 > self.buffer_base + self.buffer.len() as u64
            {
                candidate += 4;
                continue;
            }

            // Read and validate settings values
            if let Some(addr) = self.validate_play_settings_at(candidate) {
                candidates.push(addr);
            }
            candidate += 4;
        }

        if candidates.is_empty() {
            return Err(Error::OffsetSearchFailed(
                "PlaySettings not found in narrow range".to_string(),
            ));
        }

        // Select candidate closest to expected center
        candidates.sort_by_key(|&c| c.abs_diff(center));
        let selected = candidates[0];

        debug!(
            "  PlaySettings: {} candidates, selected 0x{:X} (distance from expected: {})",
            candidates.len(),
            selected,
            selected.abs_diff(center)
        );

        Ok(selected)
    }

    /// Narrow search for PlayData near PlaySettings using relative offset
    ///
    /// Search range: ±256 bytes around expected position (playSettings + 0x2C0)
    ///
    /// This method first tries known offsets (0x2C0, 0x2B0) before falling back
    /// to a full range scan. This avoids false positives from zero-filled memory
    /// at unexpected positions.
    fn search_play_data_near_settings_narrow(&mut self, play_settings: u64) -> Result<u64> {
        // Known offsets from different versions (try in order of likelihood)
        // 2025122400 and later: 0x2C0 (704 bytes)
        // Before 2025122400: 0x2B0 (688 bytes)
        const KNOWN_OFFSETS: &[u64] = &[0x2C0, 0x2B0];

        // First, try known offsets (fast path)
        // Use validate_play_data_address() which doesn't check distance from play_settings.
        // This is necessary because known offsets (e.g., 0x2C0 = 704 bytes) may exceed
        // the distance threshold in validate_play_data_at().
        for &offset in KNOWN_OFFSETS {
            let addr = play_settings + offset;
            if let Ok(true) = self.validate_play_data_address(addr) {
                debug!(
                    "  PlayData: selected 0x{:X} (known offset 0x{:X})",
                    addr, offset
                );
                return Ok(addr);
            }
        }

        // Fallback: scan around expected position
        let center = play_settings + PLAY_SETTINGS_TO_PLAY_DATA;
        let range = PLAY_DATA_SEARCH_RANGE;

        // Load buffer around the expected position
        if self.load_buffer_around(center, range).is_err() {
            return Err(Error::OffsetSearchFailed("Memory load failed".to_string()));
        }

        let base = self.reader.base_address();
        let start = center.saturating_sub(range as u64).max(base);
        let end = center.saturating_add(range as u64);

        // Collect all valid candidates
        let mut candidates: Vec<u64> = Vec::new();
        let mut candidate = (start + 3) & !3;

        while candidate < end {
            if candidate < self.buffer_base
                || candidate + 12 > self.buffer_base + self.buffer.len() as u64
            {
                candidate += 4;
                continue;
            }

            // PlayData validation: check if it looks like valid play data
            // (song_id should be reasonable, difficulty 0-9, etc.)
            if self.validate_play_data_at(candidate, play_settings) {
                candidates.push(candidate);
            }
            candidate += 4;
        }

        if candidates.is_empty() {
            return Err(Error::OffsetSearchFailed(
                "PlayData not found in narrow range".to_string(),
            ));
        }

        // Select candidate closest to expected center
        candidates.sort_by_key(|&c| c.abs_diff(center));
        let selected = candidates[0];

        debug!(
            "  PlayData: {} candidates, selected 0x{:X} (fallback scan, distance from expected: {})",
            candidates.len(),
            selected,
            selected.abs_diff(center)
        );

        Ok(selected)
    }

    /// Narrow search for CurrentSong near JudgeData using relative offset
    ///
    /// Search range: ±256 bytes around expected position (judgeData + 0x1E4)
    fn search_current_song_near_judge_narrow(
        &mut self,
        judge_data: u64,
        play_data: u64,
    ) -> Result<u64> {
        let center = judge_data + JUDGE_TO_CURRENT_SONG;
        let range = CURRENT_SONG_SEARCH_RANGE;

        if self.load_buffer_around(center, range).is_err() {
            return Err(Error::OffsetSearchFailed("Memory load failed".to_string()));
        }

        let base = self.reader.base_address();
        let start = center.saturating_sub(range as u64).max(base);
        let end = center.saturating_add(range as u64);

        // Collect all valid candidates
        let mut candidates: Vec<u64> = Vec::new();
        let mut candidate = (start + 3) & !3;

        while candidate < end {
            if candidate < self.buffer_base
                || candidate + 8 > self.buffer_base + self.buffer.len() as u64
            {
                candidate += 4;
                continue;
            }

            // CurrentSong should have the same song_id as PlayData
            if self.validate_current_song_at(candidate, play_data) {
                candidates.push(candidate);
            }
            candidate += 4;
        }

        if candidates.is_empty() {
            return Err(Error::OffsetSearchFailed(
                "CurrentSong not found in narrow range".to_string(),
            ));
        }

        // Select candidate closest to expected center
        candidates.sort_by_key(|&c| c.abs_diff(center));
        let selected = candidates[0];

        debug!(
            "  CurrentSong: {} candidates, selected 0x{:X} (distance from expected: {})",
            candidates.len(),
            selected,
            selected.abs_diff(center)
        );

        Ok(selected)
    }

    /// Validate if the given address contains valid PlaySettings
    fn validate_play_settings_at(&self, addr: u64) -> Option<u64> {
        let style = self.reader.read_i32(addr).ok()?;
        let gauge = self.reader.read_i32(addr + 4).ok()?;
        let assist = self.reader.read_i32(addr + 8).ok()?;
        let unknown = self.reader.read_i32(addr + 12).ok()?;
        let range = self.reader.read_i32(addr + 16).ok()?;

        // Valid ranges check
        if (0..=7).contains(&style)
            && (0..=5).contains(&gauge)
            && (0..=3).contains(&assist)
            && (0..=1).contains(&unknown)
            && (0..=4).contains(&range)
        {
            Some(addr)
        } else {
            None
        }
    }

    /// Validate if the given address contains valid PlayData
    fn validate_play_data_at(&self, addr: u64, play_settings: u64) -> bool {
        // PlayData should be close to PlaySettings (within expected range)
        let distance = addr.abs_diff(play_settings);

        if distance > PLAY_DATA_SEARCH_RANGE as u64 * 2 {
            return false;
        }

        // Read song_id, difficulty, ex_score, miss_count - should be reasonable values
        let song_id = self.reader.read_i32(addr).unwrap_or(-1);
        let difficulty = self.reader.read_i32(addr + 4).unwrap_or(-1);
        let ex_score = self.reader.read_i32(addr + 8).unwrap_or(-1);
        let miss_count = self.reader.read_i32(addr + 12).unwrap_or(-1);

        // Accept initial state (all zeros) - common when not in song select
        if song_id == 0 && difficulty == 0 && ex_score == 0 && miss_count == 0 {
            return true;
        }

        // Song ID should be in valid IIDX range (>= 1000)
        // Difficulty should be 0-9 (10 difficulty levels)
        (MIN_SONG_ID..=MAX_SONG_ID).contains(&song_id) && (0..=9).contains(&difficulty)
    }

    /// Validate if the given address contains PlayData in initial state (all zeros)
    ///
    /// This is used for chain validation during JudgeData detection.
    /// In song select state, PlayData should be all zeros.
    fn validate_play_data_initial_state(&self, addr: u64) -> bool {
        let song_id = self.reader.read_i32(addr).unwrap_or(-1);
        let difficulty = self.reader.read_i32(addr + 4).unwrap_or(-1);
        let ex_score = self.reader.read_i32(addr + 8).unwrap_or(-1);
        let miss_count = self.reader.read_i32(addr + 12).unwrap_or(-1);

        // Initial state: all fields are zero
        song_id == 0 && difficulty == 0 && ex_score == 0 && miss_count == 0
    }

    /// Validate if the given address contains valid CurrentSong data
    fn validate_current_song_at(&self, addr: u64, play_data: u64) -> bool {
        let current_song_id = self.reader.read_i32(addr).unwrap_or(-1);
        let play_data_song_id = self.reader.read_i32(play_data).unwrap_or(-2);

        // Accept initial state (both are zeros)
        if current_song_id == 0 && play_data_song_id == 0 {
            return true;
        }

        // Song IDs should match, be in valid range (1000-50000), and not be power of 2
        let is_valid_range = (1000..=50000).contains(&current_song_id);
        let is_power_of_two = is_power_of_two(current_song_id as u32);

        is_valid_range && !is_power_of_two && current_song_id == play_data_song_id
    }

    /// Search for JudgeData using initial state pattern (72 zero bytes)
    ///
    /// In song select state, JudgeData contains all zeros for the first 72 bytes
    /// (P1/P2 judgments, combo breaks, fast/slow, measure ends).
    /// We validate candidates by checking STATE_MARKER positions.
    fn search_judge_data_initial_state(&mut self, data_map_hint: u64) -> Result<u64> {
        let mut search_size = INITIAL_SEARCH_SIZE;

        while search_size <= MAX_SEARCH_SIZE {
            self.load_buffer_around(data_map_hint, search_size)?;

            let zero_pattern = vec![0u8; judge::INITIAL_ZERO_SIZE];
            let candidates = self.find_all_matches(&zero_pattern);

            // Filter candidates by STATE_MARKER validation
            for candidate in &candidates {
                // Read STATE_MARKERs - in song select state, both should be 0
                let marker1 = self
                    .reader
                    .read_i32(candidate + judge::STATE_MARKER_1)
                    .unwrap_or(-1);
                let marker2 = self
                    .reader
                    .read_i32(candidate + judge::STATE_MARKER_2)
                    .unwrap_or(-1);

                // Validate: in song select, both markers should be 0
                if marker1 != 0 || marker2 != 0 {
                    continue;
                }

                // Additional validation: check distance from DataMap
                // Typically judge_data is 2-10 MB away from data_map
                let distance = (*candidate as i64 - data_map_hint as i64).abs();
                if !(1_000_000..=15_000_000).contains(&distance) {
                    continue;
                }

                return Ok(*candidate);
            }

            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(
            "JudgeData not found with initial state pattern".to_string(),
        ))
    }

    /// Search for JudgeData during play using value range validation
    ///
    /// This method can detect JudgeData even when a song is being played or has been played.
    /// It validates candidates using:
    /// 1. Distance constraint from DataMap (1-15 MB)
    /// 2. Value range validation (each judgment 0-3000)
    /// 3. Consistency check (combo_break <= total, fast+slow <= total)
    /// 4. P1/P2 exclusivity (in SP, one side should be all zeros)
    fn search_judge_data_during_play(&mut self, data_map_hint: u64) -> Result<u64> {
        let mut search_size = INITIAL_SEARCH_SIZE;

        while search_size <= MAX_SEARCH_SIZE {
            self.load_buffer_around(data_map_hint, search_size)?;

            // Scan memory with 4-byte alignment within distance constraint
            let min_distance: i64 = 1_000_000;
            let max_distance: i64 = 15_000_000;

            let search_start = data_map_hint.saturating_sub(max_distance as u64);
            let search_end = data_map_hint + max_distance as u64;

            // Align to 4 bytes
            let aligned_start = (search_start + 3) & !3;

            let mut candidate = aligned_start;
            while candidate < search_end {
                // Check distance constraint
                let distance = (candidate as i64 - data_map_hint as i64).abs();
                if distance < min_distance {
                    candidate += 4;
                    continue;
                }

                if self.validate_judge_data_during_play(candidate) {
                    return Ok(candidate);
                }

                candidate += 4;
            }

            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(
            "JudgeData not found with during-play pattern".to_string(),
        ))
    }

    /// Validate a candidate address as JudgeData during play
    fn validate_judge_data_during_play(&self, addr: u64) -> bool {
        // Read P1 judgment values
        let p1_pgreat = self.reader.read_i32(addr + judge::P1_PGREAT).unwrap_or(-1);
        let p1_great = self.reader.read_i32(addr + judge::P1_GREAT).unwrap_or(-1);
        let p1_good = self.reader.read_i32(addr + judge::P1_GOOD).unwrap_or(-1);
        let p1_bad = self.reader.read_i32(addr + judge::P1_BAD).unwrap_or(-1);
        let p1_poor = self.reader.read_i32(addr + judge::P1_POOR).unwrap_or(-1);

        // Read P2 judgment values
        let p2_pgreat = self.reader.read_i32(addr + judge::P2_PGREAT).unwrap_or(-1);
        let p2_great = self.reader.read_i32(addr + judge::P2_GREAT).unwrap_or(-1);
        let p2_good = self.reader.read_i32(addr + judge::P2_GOOD).unwrap_or(-1);
        let p2_bad = self.reader.read_i32(addr + judge::P2_BAD).unwrap_or(-1);
        let p2_poor = self.reader.read_i32(addr + judge::P2_POOR).unwrap_or(-1);

        // Read combo break and fast/slow
        let p1_cb = self
            .reader
            .read_i32(addr + judge::P1_COMBO_BREAK)
            .unwrap_or(-1);
        let p2_cb = self
            .reader
            .read_i32(addr + judge::P2_COMBO_BREAK)
            .unwrap_or(-1);
        let p1_fast = self.reader.read_i32(addr + judge::P1_FAST).unwrap_or(-1);
        let p2_fast = self.reader.read_i32(addr + judge::P2_FAST).unwrap_or(-1);
        let p1_slow = self.reader.read_i32(addr + judge::P1_SLOW).unwrap_or(-1);
        let p2_slow = self.reader.read_i32(addr + judge::P2_SLOW).unwrap_or(-1);

        // Range validation for all values
        let p1_judgments = [p1_pgreat, p1_great, p1_good, p1_bad, p1_poor];
        let p2_judgments = [p2_pgreat, p2_great, p2_good, p2_bad, p2_poor];

        for &v in &p1_judgments {
            if !(0..=judge::MAX_NOTES).contains(&v) {
                return false;
            }
        }
        for &v in &p2_judgments {
            if !(0..=judge::MAX_NOTES).contains(&v) {
                return false;
            }
        }

        // Combo break range
        if !(0..=judge::MAX_COMBO_BREAK).contains(&p1_cb)
            || !(0..=judge::MAX_COMBO_BREAK).contains(&p2_cb)
        {
            return false;
        }

        // Fast/slow range
        if !(0..=judge::MAX_FAST_SLOW).contains(&p1_fast)
            || !(0..=judge::MAX_FAST_SLOW).contains(&p2_fast)
            || !(0..=judge::MAX_FAST_SLOW).contains(&p1_slow)
            || !(0..=judge::MAX_FAST_SLOW).contains(&p2_slow)
        {
            return false;
        }

        // Calculate totals
        let p1_total: i32 = p1_judgments.iter().sum();
        let p2_total: i32 = p2_judgments.iter().sum();

        // Total should not exceed MAX_NOTES
        if p1_total > judge::MAX_NOTES || p2_total > judge::MAX_NOTES {
            return false;
        }

        // Consistency check: combo_break <= total
        if p1_total > 0 && p1_cb > p1_total {
            return false;
        }
        if p2_total > 0 && p2_cb > p2_total {
            return false;
        }

        // Consistency check: fast + slow <= total
        if p1_total > 0 && (p1_fast + p1_slow) > p1_total {
            return false;
        }
        if p2_total > 0 && (p2_fast + p2_slow) > p2_total {
            return false;
        }

        // P1/P2 exclusivity check (for SP mode)
        // At least one side should have data, or both should be zero (initial state)
        let p1_all_zero = p1_total == 0 && p1_cb == 0 && p1_fast == 0 && p1_slow == 0;
        let p2_all_zero = p2_total == 0 && p2_cb == 0 && p2_fast == 0 && p2_slow == 0;

        // Valid patterns:
        // 1. Both zero (initial state)
        // 2. P1 has data, P2 is zero (1P mode)
        // 3. P1 is zero, P2 has data (2P mode)
        // 4. Both have data (DP mode)
        // All patterns are valid, but we need at least some structure
        // Reject if we have partial zeros in unexpected places
        if p1_all_zero && p2_all_zero {
            // Initial state - this should be caught by initial_state search
            // but we accept it here as well
            return true;
        }

        // For SP, one side should be all zero
        // For DP, both sides should have data
        // We accept both patterns
        true
    }

    /// Fallback: search PlaySettings by validating setting values only
    ///
    /// Used when marker search fails (e.g., during play when marker != 1).
    /// Scans memory near JudgeData and validates setting values.
    fn search_play_settings_by_values(&mut self, judge_data_hint: u64) -> Result<u64> {
        let mut search_size = INITIAL_SEARCH_SIZE;
        let expected = judge_data_hint.saturating_sub(JUDGE_TO_PLAY_SETTINGS);

        while search_size <= MAX_SEARCH_SIZE {
            self.load_buffer_around(judge_data_hint, search_size)?;

            // Distance constraint from JudgeData
            let min_distance: i64 = 100_000;
            let max_distance: i64 = 10_000_000;

            let search_start = judge_data_hint.saturating_sub(max_distance as u64);
            let search_end = judge_data_hint + max_distance as u64;

            // Align to 4 bytes
            let aligned_start = (search_start + 3) & !3;

            // Collect all valid candidates
            let mut candidates: Vec<u64> = Vec::new();
            let mut candidate = aligned_start;

            while candidate < search_end {
                // Check distance constraint
                let distance = (candidate as i64 - judge_data_hint as i64).abs();
                if distance < min_distance {
                    candidate += 4;
                    continue;
                }

                // Validate setting values at this address
                let style = self.reader.read_i32(candidate).unwrap_or(-1);
                let gauge = self.reader.read_i32(candidate + 4).unwrap_or(-1);
                let assist = self.reader.read_i32(candidate + 8).unwrap_or(-1);
                let unknown = self.reader.read_i32(candidate + 12).unwrap_or(-1);
                let range = self.reader.read_i32(candidate + 16).unwrap_or(-1);

                // Valid ranges check
                if (0..=7).contains(&style)
                    && (0..=5).contains(&gauge)
                    && (0..=3).contains(&assist)
                    && (0..=1).contains(&unknown)
                    && (0..=4).contains(&range)
                {
                    candidates.push(candidate);
                }

                candidate += 4;
            }

            if !candidates.is_empty() {
                // Select candidate closest to expected position
                candidates.sort_by_key(|&c| c.abs_diff(expected));
                let selected = candidates[0];
                debug!(
                    "  PlaySettings: {} candidates, selected 0x{:X} (fallback, distance from expected: {})",
                    candidates.len(),
                    selected,
                    selected.abs_diff(expected)
                );
                return Ok(selected);
            }

            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(
            "PlaySettings not found (both marker and fallback methods failed)".to_string(),
        ))
    }

    /// Search for PlayData near PlaySettings
    ///
    /// PlayData is typically located about 0x2B0-0x2C0 bytes after PlaySettings,
    /// but this offset varies between game versions.
    fn search_play_data_near_settings(&mut self, play_settings: u64) -> Result<u64> {
        // Known offsets from different versions (try in order of likelihood)
        // 2025122400: 0x2C0 (704)
        // 2024-2025 (before 2025122400): 0x2B0 (688)
        const KNOWN_OFFSETS: &[u64] = &[0x2C0, 0x2B0];

        // First, try known offsets
        for &offset in KNOWN_OFFSETS {
            let addr = play_settings + offset;
            if let Ok(true) = self.validate_play_data_address(addr) {
                return Ok(addr);
            }
        }

        // Fallback: scan around expected locations
        let center = play_settings + 0x2B0; // Use older offset as center
        let tolerance: u64 = 100; // 100 bytes should be enough

        self.load_buffer_around(center, tolerance as usize * 2)?;

        // Search from center outward, checking 4-byte aligned addresses
        for delta in (0..=tolerance).step_by(4) {
            for &sign in &[1i64, -1i64] {
                let addr = center.wrapping_add_signed(sign * delta as i64);
                if addr <= play_settings + 256 {
                    continue;
                }

                if let Ok(true) = self.validate_play_data_address(addr) {
                    return Ok(addr);
                }
            }
        }

        Err(Error::OffsetSearchFailed(
            "PlayData not found near PlaySettings".to_string(),
        ))
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

    /// Search for CurrentSong near JudgeData
    ///
    /// CurrentSong is typically located about 0x1E4-0x1F4 bytes after JudgeData,
    /// but this offset varies between game versions.
    fn search_current_song_near_judge(&mut self, judge_data: u64, play_data: u64) -> Result<u64> {
        // Known offsets from different versions (try in order of likelihood)
        // 2025122400: 0x1E4 (484)
        // 2024-2025 (before 2025122400): 0x1F4 (500)
        const KNOWN_OFFSETS: &[u64] = &[0x1E4, 0x1F4];

        // First, try known offsets
        for &offset in KNOWN_OFFSETS {
            let addr = judge_data + offset;

            // Ensure this isn't the same as PlayData
            let play_data_distance = (addr as i64 - play_data as i64).unsigned_abs();
            if play_data_distance < 256 {
                continue;
            }

            if let Ok(true) = self.validate_current_song_address(addr) {
                return Ok(addr);
            }
        }

        // Fallback: scan around expected locations
        let center = judge_data + 0x1F0; // Midpoint between known offsets
        let tolerance: u64 = 100;

        self.load_buffer_around(center, tolerance as usize * 2)?;

        for delta in (0..=tolerance).step_by(4) {
            for &sign in &[1i64, -1i64] {
                let addr = center.wrapping_add_signed(sign * delta as i64);

                // Ensure this isn't the same as PlayData
                let play_data_distance = (addr as i64 - play_data as i64).unsigned_abs();
                if play_data_distance < 256 {
                    continue;
                }

                if let Ok(true) = self.validate_current_song_address(addr) {
                    return Ok(addr);
                }
            }
        }

        Err(Error::OffsetSearchFailed(
            "CurrentSong not found near JudgeData".to_string(),
        ))
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

    /// Validate offsets by searching for code references
    ///
    /// This searches for x64 RIP-relative addressing instructions (LEA, MOV)
    /// that reference the found offsets. If found, it increases confidence.
    fn validate_offsets_with_signatures(&mut self, offsets: &OffsetsCollection) -> usize {
        let mut matches = 0;

        // Load code section for signature search
        let code_base = self.reader.base_address();
        if self
            .load_buffer_around(code_base, 50 * 1024 * 1024)
            .is_err()
        {
            debug!("Failed to load code section for signature validation");
            return 0;
        }

        // Collect all addresses to validate
        let addresses = [
            ("JudgeData", offsets.judge_data),
            ("PlayData", offsets.play_data),
            ("PlaySettings", offsets.play_settings),
            ("CurrentSong", offsets.current_song),
        ];

        for (name, addr) in addresses {
            if self.find_code_reference(addr) {
                debug!("    {} (0x{:X}) validated by code reference", name, addr);
                matches += 1;
            }
        }

        matches
    }

    /// Search for code that references a specific data address
    ///
    /// Looks for x64 RIP-relative LEA/MOV instructions.
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

    /// Dump current values at detected offsets for verification (compact format)
    fn dump_offset_values(&self, offsets: &OffsetsCollection) {
        // JudgeData - P1/P2 judgment counts in compact format
        let p1 = [
            self.reader
                .read_i32(offsets.judge_data + judge::P1_PGREAT)
                .unwrap_or(-1),
            self.reader
                .read_i32(offsets.judge_data + judge::P1_GREAT)
                .unwrap_or(-1),
            self.reader
                .read_i32(offsets.judge_data + judge::P1_GOOD)
                .unwrap_or(-1),
            self.reader
                .read_i32(offsets.judge_data + judge::P1_BAD)
                .unwrap_or(-1),
            self.reader
                .read_i32(offsets.judge_data + judge::P1_POOR)
                .unwrap_or(-1),
        ];
        let p2 = [
            self.reader
                .read_i32(offsets.judge_data + judge::P2_PGREAT)
                .unwrap_or(-1),
            self.reader
                .read_i32(offsets.judge_data + judge::P2_GREAT)
                .unwrap_or(-1),
            self.reader
                .read_i32(offsets.judge_data + judge::P2_GOOD)
                .unwrap_or(-1),
            self.reader
                .read_i32(offsets.judge_data + judge::P2_BAD)
                .unwrap_or(-1),
            self.reader
                .read_i32(offsets.judge_data + judge::P2_POOR)
                .unwrap_or(-1),
        ];
        debug!("  JudgeData: P1={:?} P2={:?}", p1, p2);

        // PlaySettings
        let style = self.reader.read_i32(offsets.play_settings).unwrap_or(-1);
        let gauge = self
            .reader
            .read_i32(offsets.play_settings + 4)
            .unwrap_or(-1);
        let assist = self
            .reader
            .read_i32(offsets.play_settings + 8)
            .unwrap_or(-1);
        let range = self
            .reader
            .read_i32(offsets.play_settings + 16)
            .unwrap_or(-1);
        debug!(
            "  PlaySettings: style={}, gauge={}, assist={}, range={}",
            style, gauge, assist, range
        );

        // PlayData
        let song_id = self.reader.read_i32(offsets.play_data).unwrap_or(-1);
        let difficulty = self.reader.read_i32(offsets.play_data + 4).unwrap_or(-1);
        let ex_score = self.reader.read_i32(offsets.play_data + 8).unwrap_or(-1);
        debug!(
            "  PlayData: song_id={}, diff={}, ex={}",
            song_id, difficulty, ex_score
        );

        // CurrentSong
        let current_song_id = self.reader.read_i32(offsets.current_song).unwrap_or(-1);
        let current_diff = self.reader.read_i32(offsets.current_song + 4).unwrap_or(-1);
        debug!(
            "  CurrentSong: song_id={}, diff={}",
            current_song_id, current_diff
        );
    }
}
