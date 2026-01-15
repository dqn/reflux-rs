use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::game::{PlayType, SongInfo};
use crate::memory::ReadMemory;
use crate::memory::layout::{judge, settings};
use crate::offset::OffsetsCollection;

const INITIAL_SEARCH_SIZE: usize = 2 * 1024 * 1024; // 2MB
const MAX_SEARCH_SIZE: usize = 300 * 1024 * 1024; // 300MB

/// Minimum number of songs expected in INFINITAS (for validation)
const MIN_EXPECTED_SONGS: usize = 1000;

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
    /// It uses multiple strategies:
    /// 1. Static pattern search (version, song_list, unlock_data, data_map)
    /// 2. Initial state pattern search (judge_data, play_settings)
    /// 3. Relative offset inference (play_data, current_song)
    /// 4. Code signature validation
    pub fn search_all(&mut self) -> Result<OffsetsCollection> {
        info!("Starting automatic offset detection...");
        let mut offsets = OffsetsCollection::default();
        let base = self.reader.base_address();

        // Phase 1: Static pattern search (high reliability)
        debug!("Phase 1: Searching static patterns...");

        // Search for version/song_list with expanding search area
        let (version, song_list) = self.search_version_and_song_list(base)?;
        offsets.version = version;
        offsets.song_list = song_list;
        info!("  Version: {}", offsets.version);
        info!("  SongList: 0x{:X}", offsets.song_list);

        offsets.unlock_data = self.search_unlock_data_offset(offsets.song_list)?;
        info!("  UnlockData: 0x{:X}", offsets.unlock_data);

        offsets.data_map = self.search_data_map_offset(offsets.song_list)?;
        info!("  DataMap: 0x{:X}", offsets.data_map);

        // Phase 2: Initial state pattern search (medium reliability)
        debug!("Phase 2: Searching initial state patterns...");

        match self.search_judge_data_initial_state(offsets.data_map) {
            Ok(addr) => {
                offsets.judge_data = addr;
                info!("  JudgeData: 0x{:X}", offsets.judge_data);
            }
            Err(e) => {
                warn!("  JudgeData search failed: {}", e);
                return Err(e);
            }
        }

        match self.search_play_settings_from_marker(offsets.judge_data) {
            Ok(addr) => {
                offsets.play_settings = addr;
                info!("  PlaySettings: 0x{:X}", offsets.play_settings);
            }
            Err(e) => {
                warn!("  PlaySettings search failed: {}", e);
                return Err(e);
            }
        }

        // Phase 3: Nearby search (search near found offsets)
        debug!("Phase 3: Searching nearby offsets...");

        match self.search_play_data_near_settings(offsets.play_settings) {
            Ok(addr) => {
                offsets.play_data = addr;
                info!("  PlayData: 0x{:X}", offsets.play_data);
            }
            Err(e) => {
                warn!("  PlayData search failed: {}", e);
                return Err(e);
            }
        }

        // Search for CurrentSong near JudgeData (offset varies by version)
        match self.search_current_song_near_judge(offsets.judge_data, offsets.play_data) {
            Ok(addr) => {
                offsets.current_song = addr;
                info!("  CurrentSong: 0x{:X}", offsets.current_song);
            }
            Err(e) => {
                warn!("  CurrentSong search failed: {}", e);
                return Err(e);
            }
        }

        // Phase 4: Validation
        debug!("Phase 4: Validating offsets...");
        if !offsets.is_valid() {
            warn!("Offset validation failed");
            return Err(Error::OffsetSearchFailed(
                "Validation failed: some offsets are zero".to_string(),
            ));
        }

        // Additional sanity checks for offset positions
        // songList should typically be at least 20MB from base
        let song_list_offset = offsets.song_list.saturating_sub(base);
        if song_list_offset < 20 * 1024 * 1024 {
            warn!(
                "songList offset seems too low: 0x{:X} ({}MB from base). This may indicate wrong detection.",
                offsets.song_list,
                song_list_offset / 1024 / 1024
            );
            return Err(Error::OffsetSearchFailed(format!(
                "songList offset too low: 0x{:X} (expected >= 20MB from base)",
                offsets.song_list
            )));
        }

        // Phase 5: Code signature validation (optional, for increased confidence)
        debug!("Phase 5: Code signature validation...");
        let signature_matches = self.validate_offsets_with_signatures(&offsets);
        if signature_matches > 0 {
            info!(
                "  Validated {} offset(s) with code signatures",
                signature_matches
            );
        } else {
            debug!("  No code signature matches found (this is OK)");
        }

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
    ///
    /// Uses last match to avoid false positives from earlier memory regions.
    pub fn search_data_map_offset(&mut self, base_hint: u64) -> Result<u64> {
        // Pattern: 0x7FFF, 0 (markers for hash map)
        let pattern = merge_byte_representations(&[0x7FFF, 0]);
        // Offset back 3 steps in 8-byte address space
        self.fetch_and_search_last(base_hint, &pattern, -24)
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
        debug!(
            "  Found {} match(es), using last at 0x{:X}",
            last_matches.len(),
            address
        );
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

        // Read up to a reasonable limit to avoid infinite loops
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
    /// This method searches for "P2D:J:B:A:" pattern and validates candidates
    /// by actually reading the song list to ensure we found the correct offset.
    fn search_version_and_song_list(&mut self, base_hint: u64) -> Result<(String, u64)> {
        let pattern = b"P2D:J:B:A:";
        let mut search_size = INITIAL_SEARCH_SIZE;
        let mut all_matches: Vec<u64> = Vec::new();

        // Progressively expand search area until memory read fails
        while search_size <= MAX_SEARCH_SIZE {
            debug!(
                "  Searching for version string in {}MB area...",
                search_size / 1024 / 1024
            );

            match self.load_buffer_around(base_hint, search_size) {
                Ok(()) => {
                    all_matches = self.find_all_matches(pattern);
                    debug!(
                        "    Found {} match(es) in {}MB",
                        all_matches.len(),
                        search_size / 1024 / 1024
                    );
                }
                Err(e) => {
                    debug!(
                        "  Memory read failed at {}MB ({}), using previous results",
                        search_size / 1024 / 1024,
                        e
                    );
                    break;
                }
            }

            search_size *= 2;
        }

        if all_matches.is_empty() {
            return Err(Error::OffsetSearchFailed(
                "Version string not found within search area".to_string(),
            ));
        }

        debug!(
            "  Found {} total candidate(s), validating by song count...",
            all_matches.len()
        );

        // Try candidates from last to first (newer versions tend to appear later)
        // Validate each by counting readable songs
        for (idx, &candidate) in all_matches.iter().rev().enumerate() {
            let song_count = self.count_songs_at_address(candidate);
            debug!(
                "    Candidate {} (0x{:X}): {} songs readable",
                all_matches.len() - idx,
                candidate,
                song_count
            );

            if song_count >= MIN_EXPECTED_SONGS {
                info!(
                    "  Validated SongList at 0x{:X} with {} songs",
                    candidate, song_count
                );

                // Extract version string
                let pos = (candidate - self.buffer_base) as usize;
                let end = self.buffer[pos..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| pos + p)
                    .unwrap_or(pos + 30);

                let version_bytes = &self.buffer[pos..end.min(pos + 30)];
                let version = String::from_utf8_lossy(version_bytes).to_string();

                return Ok((version, candidate));
            }
        }

        // If no candidate passed validation, return an error with diagnostic info
        let candidates_info: Vec<String> = all_matches
            .iter()
            .rev()
            .take(5)
            .map(|&addr| {
                let count = self.count_songs_at_address(addr);
                format!("0x{:X} ({} songs)", addr, count)
            })
            .collect();

        Err(Error::OffsetSearchFailed(format!(
            "No SongList candidate passed validation (>= {} songs). Candidates: {}",
            MIN_EXPECTED_SONGS,
            candidates_info.join(", ")
        )))
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

/// Trait for interactive user prompts during offset search
pub trait SearchPrompter {
    /// Prompt user to press enter to continue
    fn prompt_continue(&self, message: &str);

    /// Prompt user to enter a number
    fn prompt_number(&self, prompt: &str) -> u32;

    /// Display a message to the user
    fn display_message(&self, message: &str);

    /// Display a warning message
    fn display_warning(&self, message: &str);
}

/// Interactive offset search result
#[derive(Debug, Clone)]
pub struct InteractiveSearchResult {
    pub offsets: OffsetsCollection,
    pub play_type: PlayType,
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

    /// Search for JudgeData using initial state pattern (72 zero bytes)
    ///
    /// In song select state, JudgeData contains all zeros for the first 72 bytes
    /// (P1/P2 judgments, combo breaks, fast/slow, measure ends).
    /// We validate candidates by checking STATE_MARKER positions.
    fn search_judge_data_initial_state(&mut self, data_map_hint: u64) -> Result<u64> {
        debug!("Searching JudgeData with initial state pattern...");

        // Expand search area
        let mut search_size = INITIAL_SEARCH_SIZE;

        while search_size <= MAX_SEARCH_SIZE {
            self.load_buffer_around(data_map_hint, search_size)?;

            let zero_pattern = vec![0u8; judge::INITIAL_ZERO_SIZE];
            let candidates = self.find_all_matches(&zero_pattern);
            debug!(
                "  Found {} zero pattern candidates in {}MB",
                candidates.len(),
                search_size / 1024 / 1024
            );

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
                    debug!(
                        "    0x{:X} rejected: distance from DataMap = {} bytes",
                        candidate, distance
                    );
                    continue;
                }

                debug!(
                    "    0x{:X} validated: markers=({}, {}), distance={}",
                    candidate, marker1, marker2, distance
                );
                return Ok(*candidate);
            }

            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(
            "JudgeData not found with initial state pattern".to_string(),
        ))
    }

    /// Search for PlaySettings using song_select_marker
    ///
    /// In song select state, the marker at (PlaySettings - 24) equals 1.
    /// We search for this marker and validate the settings values.
    fn search_play_settings_from_marker(&mut self, judge_data_hint: u64) -> Result<u64> {
        debug!("Searching PlaySettings using song_select_marker...");

        let mut search_size = INITIAL_SEARCH_SIZE;
        let mut best_candidate: Option<u64> = None;

        while search_size <= MAX_SEARCH_SIZE {
            self.load_buffer_around(judge_data_hint, search_size)?;

            // Search for song_select_marker == 1
            // Note: This value is common in memory, so we validate thoroughly
            let marker_pattern = 1i32.to_le_bytes().to_vec();
            let candidates = self.find_all_matches(&marker_pattern);
            debug!(
                "  Found {} marker candidates in {}MB",
                candidates.len(),
                search_size / 1024 / 1024
            );

            for marker_addr in &candidates {
                // PlaySettings is at marker_addr + 24
                let play_settings = marker_addr + settings::SONG_SELECT_MARKER;

                // Validate: read settings values and check if they're in valid range
                let style = self.reader.read_i32(play_settings).unwrap_or(-1);
                let gauge = self.reader.read_i32(play_settings + 4).unwrap_or(-1);
                let assist = self.reader.read_i32(play_settings + 8).unwrap_or(-1);
                let unknown = self.reader.read_i32(play_settings + 12).unwrap_or(-1);
                let range = self.reader.read_i32(play_settings + 16).unwrap_or(-1);

                // Valid ranges (matching actual game options):
                // - style: 0-7 (OFF, RANDOM, R-RANDOM, S-RANDOM, MIRROR, etc.)
                // - gauge: 0-5 (NORMAL, EASY, HARD, EX-HARD, ASSISTED-EASY, etc.)
                // - assist: 0-3 (OFF, AUTO-SCRATCH, LEGACY-NOTE, A-SCR+LEGACY)
                // - unknown: should be 0 in most cases
                // - range: 0-4 (OFF, SUDDEN+, HIDDEN+, SUD+HID+, LIFT)
                if !(0..=7).contains(&style) {
                    continue;
                }
                if !(0..=5).contains(&gauge) {
                    continue;
                }
                if !(0..=3).contains(&assist) {
                    continue;
                }
                if !(0..=1).contains(&unknown) {
                    continue;
                }
                if !(0..=4).contains(&range) {
                    continue;
                }

                // Additional validation: check distance from JudgeData
                let distance = (play_settings as i64 - judge_data_hint as i64).abs();
                if !(100_000..=10_000_000).contains(&distance) {
                    continue;
                }

                debug!(
                    "    0x{:X} validated: style={}, gauge={}, assist={}, range={}, distance={}",
                    play_settings, style, gauge, assist, range, distance
                );

                // Keep track of last valid match (similar to version string issue)
                best_candidate = Some(play_settings);
            }

            // If we found candidates in this search size, return the last one
            if let Some(addr) = best_candidate {
                return Ok(addr);
            }

            search_size *= 2;
        }

        Err(Error::OffsetSearchFailed(
            "PlaySettings not found with marker pattern".to_string(),
        ))
    }

    /// Search for PlayData near PlaySettings
    ///
    /// PlayData is typically located about 0x2B0-0x2C0 bytes after PlaySettings,
    /// but this offset varies between game versions.
    fn search_play_data_near_settings(&mut self, play_settings: u64) -> Result<u64> {
        debug!("Searching PlayData near PlaySettings...");

        // Known offsets from different versions (try in order of likelihood)
        // 2025122400: 0x2C0 (704)
        // 2024-2025 (before 2025122400): 0x2B0 (688)
        const KNOWN_OFFSETS: &[u64] = &[0x2C0, 0x2B0];

        // First, try known offsets
        for &offset in KNOWN_OFFSETS {
            let addr = play_settings + offset;
            debug!("  Trying known offset 0x{:X} -> 0x{:X}", offset, addr);

            if let Ok(true) = self.validate_play_data_address(addr) {
                info!(
                    "  PlayData found at known offset 0x{:X} from PlaySettings",
                    offset
                );
                return Ok(addr);
            }
        }

        // Fallback: scan around expected locations
        debug!("  Known offsets failed, searching nearby...");
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
                    info!(
                        "  PlayData found at 0x{:X} (offset 0x{:X} from PlaySettings)",
                        addr,
                        addr - play_settings
                    );
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

        // Accept initial state (all zeros) - game hasn't played any song yet
        let is_initial_state = song_id == 0 && difficulty == 0 && ex_score == 0 && miss_count == 0;

        // Accept valid play data
        let is_valid_play_data = (0..=50000).contains(&song_id)
            && (0..=9).contains(&difficulty)
            && (0..=10000).contains(&ex_score)
            && (0..=3000).contains(&miss_count);

        if is_initial_state || is_valid_play_data {
            debug!(
                "    0x{:X}: song_id={}, diff={}, ex={}, miss={} ({})",
                addr,
                song_id,
                difficulty,
                ex_score,
                miss_count,
                if is_initial_state { "initial" } else { "valid" }
            );
            return Ok(true);
        }

        Ok(false)
    }

    /// Search for CurrentSong near JudgeData
    ///
    /// CurrentSong is typically located about 0x1E4-0x1F4 bytes after JudgeData,
    /// but this offset varies between game versions.
    fn search_current_song_near_judge(&mut self, judge_data: u64, play_data: u64) -> Result<u64> {
        debug!("Searching CurrentSong near JudgeData...");

        // Known offsets from different versions (try in order of likelihood)
        // 2025122400: 0x1E4 (484)
        // 2024-2025 (before 2025122400): 0x1F4 (500)
        const KNOWN_OFFSETS: &[u64] = &[0x1E4, 0x1F4];

        // First, try known offsets
        for &offset in KNOWN_OFFSETS {
            let addr = judge_data + offset;
            debug!("  Trying known offset 0x{:X} -> 0x{:X}", offset, addr);

            // Ensure this isn't the same as PlayData
            let play_data_distance = (addr as i64 - play_data as i64).unsigned_abs();
            if play_data_distance < 256 {
                continue;
            }

            if let Ok(true) = self.validate_current_song_address(addr) {
                info!(
                    "  CurrentSong found at known offset 0x{:X} from JudgeData",
                    offset
                );
                return Ok(addr);
            }
        }

        // Fallback: scan around expected locations
        debug!("  Known offsets failed, searching nearby...");
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
                    info!(
                        "  CurrentSong found at 0x{:X} (offset 0x{:X} from JudgeData)",
                        addr,
                        addr - judge_data
                    );
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
            debug!("    0x{:X}: initial state (zeros)", addr);
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

        debug!(
            "    0x{:X}: song_id={}, difficulty={}, field3={} (valid)",
            addr, song_id, difficulty, field3
        );
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
                    // Extract RIP-relative offset
                    let rel_offset = i32::from_le_bytes(window[3..7].try_into().unwrap_or([0; 4]));

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

/// Convert i32 values to little-endian byte representation
pub fn merge_byte_representations(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Check if a number is a power of two (used to filter out memory artifacts)
fn is_power_of_two(n: u32) -> bool {
    n > 0 && (n & (n - 1)) == 0
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
