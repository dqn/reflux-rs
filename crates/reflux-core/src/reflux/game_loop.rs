//! Game loop and state handling for Reflux
//!
//! This module contains the main tracking loop and game state handling methods.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use tracing::{debug, error, info, warn};

// =============================================================================
// Retry and polling configuration
// =============================================================================

/// Memory read retry settings.
/// Exponential backoff: 100ms → 200ms → 400ms → 800ms = total 1.5s max.
/// Longer delays reduce false disconnection detection from transient failures.
const MAX_READ_RETRIES: u32 = 5;
const RETRY_DELAYS_MS: [u64; 5] = [100, 200, 400, 800, 1600];

/// Result screen polling delays (exponential backoff).
/// Total: 50+50+100+100+200+200+300+300+500+500 = 2.3 seconds max.
/// Faster initial polling catches quick data availability, while exponential
/// backoff reduces CPU usage if data takes longer to populate.
const POLL_DELAYS_MS: [u64; 10] = [50, 50, 100, 100, 200, 200, 300, 300, 500, 500];

use crate::error::Result;
use crate::game::{
    AssistType, ChartInfo, Difficulty, GameState, Grade, Judge, Lamp, PlayData, PlayType,
    PlayerJudge, RawJudgeData, Settings, check_version_match, fetch_song_by_id,
    fetch_song_database_from_memory_scan, find_game_version, get_unlock_states,
};
use crate::memory::layout::{judge, play, settings, timing};
use crate::memory::{MemoryReader, ProcessHandle, ReadMemory};
use crate::storage::format_play_data_console;

use super::Reflux;

impl Reflux {
    /// Run the main tracking loop
    ///
    /// The `running` flag is checked each iteration to allow graceful shutdown via Ctrl+C.
    pub fn run(&mut self, process: &ProcessHandle, running: &AtomicBool) -> Result<()> {
        let reader = MemoryReader::new(process);
        let mut last_state = GameState::Unknown;

        debug!("Starting tracker loop...");

        // Start TSV session
        self.session_manager = crate::storage::SessionManager::new("sessions");
        match self.session_manager.start_tsv_session() {
            Ok(path) => debug!("Started TSV session at {:?}", path),
            Err(e) => warn!("Failed to start TSV session: {}", e),
        }

        loop {
            // Check for shutdown signal
            // Note: `running` is actually the shutdown flag from ShutdownSignal.as_atomic()
            // It's true when shutdown is requested, so we exit when it's true
            if running.load(Ordering::SeqCst) {
                debug!("Shutdown signal received, exiting tracker loop");
                break;
            }

            // Step 1: Fast check if process is still alive via exit code
            if !process.is_alive() {
                debug!("Process terminated (exit code check)");
                break;
            }

            // Step 2: Verify memory access with retry mechanism (exponential backoff)
            let mut memory_accessible = false;
            for attempt in 0..MAX_READ_RETRIES {
                match reader.read_bytes(process.base_address, 4) {
                    Ok(_) => {
                        memory_accessible = true;
                        break;
                    }
                    Err(e) => {
                        // Re-check process status before retrying
                        if !process.is_alive() {
                            debug!("Process terminated during retry: {}", e);
                            break;
                        }

                        if attempt < MAX_READ_RETRIES - 1 {
                            let delay = RETRY_DELAYS_MS[attempt as usize];
                            debug!(
                                "Memory read failed (attempt {}/{}, retry in {}ms): {}",
                                attempt + 1,
                                MAX_READ_RETRIES,
                                delay,
                                e
                            );
                            thread::sleep(Duration::from_millis(delay));
                        } else {
                            debug!(
                                "Memory read failed after {} retries: {}",
                                MAX_READ_RETRIES, e
                            );
                        }
                    }
                }
            }
            if !memory_accessible {
                break;
            }

            // Detect game state
            let current_state = self.detect_game_state(&reader)?;

            if current_state != last_state {
                debug!("State changed: {:?} -> {:?}", last_state, current_state);
                self.handle_state_change(&reader, last_state, current_state)?;
                last_state = current_state;
            }

            thread::sleep(Duration::from_millis(timing::GAME_STATE_POLL_INTERVAL_MS));
        }

        Ok(())
    }

    fn detect_game_state(&mut self, reader: &MemoryReader) -> Result<GameState> {
        // Read markers for state detection with detailed error logging
        let state_marker_1 = match reader.read_i32(self.offsets.judge_data + judge::STATE_MARKER_1)
        {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to read state_marker_1: {}", e);
                0
            }
        };
        let state_marker_2 = match reader.read_i32(self.offsets.judge_data + judge::STATE_MARKER_2)
        {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to read state_marker_2: {}", e);
                0
            }
        };
        let song_select_marker = match reader.read_i32(
            self.offsets
                .play_settings
                .wrapping_sub(settings::SONG_SELECT_MARKER),
        ) {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to read song_select_marker: {}", e);
                0
            }
        };

        Ok(self
            .state_detector
            .detect(state_marker_1, state_marker_2, song_select_marker))
    }

    fn handle_state_change(
        &mut self,
        reader: &MemoryReader,
        _old_state: GameState,
        new_state: GameState,
    ) -> Result<()> {
        match new_state {
            GameState::ResultScreen => self.handle_result_screen(reader),
            GameState::SongSelect => self.handle_song_select(reader),
            GameState::Playing => self.handle_playing(reader),
            GameState::Unknown => {}
        }
        Ok(())
    }

    /// Handle transition to result screen
    fn handle_result_screen(&mut self, reader: &MemoryReader) {
        info!("Detected result screen, waiting for data...");

        // Initial delay to allow game data to settle (matching C# implementation)
        // This prevents race conditions where judge data updates before play data
        thread::sleep(Duration::from_millis(1000));

        // Poll until play data becomes available (exponential backoff)
        for (attempt, &delay) in POLL_DELAYS_MS.iter().enumerate() {
            thread::sleep(Duration::from_millis(delay));

            match self.fetch_play_data(reader) {
                Ok(play_data) => {
                    // Verify data looks valid (non-zero total notes)
                    let total_notes = play_data.judge.pgreat
                        + play_data.judge.great
                        + play_data.judge.good
                        + play_data.judge.bad
                        + play_data.judge.poor;

                    debug!(
                        "Attempt {}: song_id={}, total_notes={}, judge: P={} G={} Go={} B={} Po={}",
                        attempt + 1,
                        play_data.chart.song_id,
                        total_notes,
                        play_data.judge.pgreat,
                        play_data.judge.great,
                        play_data.judge.good,
                        play_data.judge.bad,
                        play_data.judge.poor
                    );

                    if total_notes > 0 {
                        info!(
                            "Play result captured: {} ({}) - EX: {}",
                            play_data.chart.title, play_data.chart.song_id, play_data.ex_score
                        );
                        self.process_play_result(&play_data);
                        return;
                    }
                    // Data not ready yet, continue polling
                    if attempt == POLL_DELAYS_MS.len() - 1 {
                        debug!(
                            "Play data notes count is zero after {} attempts",
                            POLL_DELAYS_MS.len()
                        );
                    }
                }
                Err(e) => {
                    if attempt == POLL_DELAYS_MS.len() - 1 {
                        error!(
                            "Failed to fetch play data after {} attempts: {}",
                            POLL_DELAYS_MS.len(),
                            e
                        );
                    }
                }
            }
        }
    }

    /// Process and save play result data
    fn process_play_result(&mut self, play_data: &PlayData) {
        // Print detailed play data to console
        println!("{}", format_play_data_console(play_data));

        // Save to session files
        self.save_session_data(play_data);
    }

    /// Save play data to session file (TSV)
    fn save_session_data(&mut self, play_data: &PlayData) {
        debug!(
            "Saving session data: song_id={}, title={}, ex_score={}",
            play_data.chart.song_id, play_data.chart.title, play_data.ex_score
        );

        if self.session_manager.current_session_path().is_none() {
            warn!("No active TSV session, attempting to start one...");
            if let Err(e) = self.session_manager.start_tsv_session() {
                error!("Failed to start TSV session: {}", e);
                return;
            }
        }

        match self.session_manager.append_tsv_row(play_data) {
            Ok(()) => {
                if let Some(path) = self.session_manager.current_session_path() {
                    debug!("Successfully wrote to session file: {:?}", path);
                }
            }
            Err(e) => error!("Failed to append TSV row: {}", e),
        }
    }

    /// Handle transition to song select screen
    fn handle_song_select(&mut self, reader: &MemoryReader) {
        // Re-scan for newly loaded songs (handles lazy loading)
        self.rescan_song_database(reader);

        // Poll unlock state changes
        self.poll_unlock_changes(reader);

        // Export tracker.tsv
        if let Err(e) = self.export_tracker_tsv("tracker.tsv") {
            error!("Failed to export tracker.tsv: {}", e);
        }
    }

    /// Re-scan memory for newly loaded songs
    ///
    /// This handles lazy loading in newer INFINITAS versions where songs are
    /// only loaded into memory when scrolled to in the song select screen.
    fn rescan_song_database(&mut self, reader: &MemoryReader) {
        let scan_result =
            fetch_song_database_from_memory_scan(reader, self.offsets.song_list, 0x200000);

        let mut new_songs = 0usize;
        for (song_id, song) in scan_result {
            if !self.game_data.song_db.contains_key(&song_id) {
                debug!(
                    "Discovered new song via rescan: {} ({})",
                    song.title, song_id
                );
                self.game_data.song_db.insert(song_id, song);
                new_songs += 1;
            }
        }

        if new_songs > 0 {
            info!(
                "Re-scan discovered {} new songs (total: {})",
                new_songs,
                self.game_data.song_db.len()
            );
        }
    }

    /// Handle transition to playing state
    fn handle_playing(&mut self, _reader: &MemoryReader) {
        // No streaming output in this version
    }

    /// Poll for unlock state changes
    fn poll_unlock_changes(&mut self, reader: &MemoryReader) {
        if self.game_data.song_db.is_empty() {
            return;
        }

        // Read current unlock state
        let current_state =
            match get_unlock_states(reader, self.offsets.unlock_data, &self.game_data.song_db) {
                Ok(state) => state,
                Err(e) => {
                    error!("Failed to read unlock state: {}", e);
                    return;
                }
            };

        // Detect changes
        let changes =
            crate::game::detect_unlock_changes(&self.game_data.unlock_state, &current_state);

        if !changes.is_empty() {
            debug!("Detected {} unlock state changes", changes.len());
        }

        // Update current unlock state
        self.game_data.unlock_state = current_state;
    }

    /// Fetch current chart selection (for future stream output feature)
    #[allow(dead_code)]
    fn fetch_current_chart(&self, reader: &MemoryReader) -> Result<(u32, Difficulty)> {
        let song_id = reader.read_i32(self.offsets.current_song)? as u32;
        let diff = reader.read_i32(self.offsets.current_song + 4)?;

        let difficulty = Difficulty::from_u8(diff as u8).unwrap_or(Difficulty::SpN);

        Ok((song_id, difficulty))
    }

    fn fetch_play_data(&mut self, reader: &MemoryReader) -> Result<PlayData> {
        // Read data in same order as C# implementation:
        // 1. Judge data first (updates earliest on result screen)
        // 2. Settings
        // 3. PlayData last (song_id, difficulty, lamp)
        // This ordering ensures we get consistent data when transitioning to result screen,
        // since judge data updates before play data in the game.
        let judge = self.fetch_judge_data(reader)?;
        let settings = self.fetch_settings(reader, judge.play_type)?;

        // Read basic play data (after judge/settings to match C# timing)
        let song_id = reader.read_i32(self.offsets.play_data + play::SONG_ID)? as u32;
        let difficulty_val = reader.read_i32(self.offsets.play_data + play::DIFFICULTY)?;
        let lamp_val = reader.read_i32(self.offsets.play_data + play::LAMP)?;

        let difficulty = Difficulty::from_u8(difficulty_val as u8).unwrap_or(Difficulty::SpN);
        let mut lamp = Lamp::from_u8(lamp_val as u8).unwrap_or(Lamp::NoPlay);

        // Upgrade to PFC if applicable
        if judge.is_pfc() && lamp == Lamp::FullCombo {
            lamp = Lamp::Pfc;
        }

        // Calculate EX score
        let ex_score = judge.ex_score();
        let data_available =
            !settings.h_ran && !settings.battle && settings.assist == AssistType::Off;

        let chart = self.create_chart_info_dynamic(reader, song_id, difficulty);

        // Calculate grade
        let grade = if chart.total_notes > 0 {
            PlayData::calculate_grade(ex_score, chart.total_notes)
        } else {
            Grade::NoPlay
        };

        Ok(PlayData {
            timestamp: Utc::now(),
            chart,
            ex_score,
            grade,
            lamp,
            judge,
            settings,
            data_available,
        })
    }

    /// Create chart info from song database, dynamically loading from memory if not found
    fn create_chart_info_dynamic(
        &mut self,
        reader: &MemoryReader,
        song_id: u32,
        difficulty: Difficulty,
    ) -> ChartInfo {
        // First check if song is already in database
        if let Some(song) = self.game_data.song_db.get(&song_id) {
            return ChartInfo::from_song_info(song, difficulty, true);
        }

        // Try to dynamically load from memory
        if let Some(song) = fetch_song_by_id(reader, self.offsets.song_list, song_id, 0x200000) {
            info!(
                "Dynamically loaded song: {} ({})",
                song.title, song_id
            );
            let chart = ChartInfo::from_song_info(&song, difficulty, true);
            // Add to song database for future lookups
            self.game_data.song_db.insert(song_id, song);
            return chart;
        }

        // Fallback to placeholder
        debug!("Song {} not found in memory, using placeholder", song_id);
        ChartInfo {
            song_id,
            title: format!("Song {:05}", song_id).into(),
            title_english: format!("Song {:05}", song_id).into(),
            artist: "".into(),
            genre: "".into(),
            bpm: "".into(),
            difficulty,
            level: 0,
            total_notes: 0,
            unlocked: true,
        }
    }

    fn fetch_judge_data(&self, reader: &MemoryReader) -> Result<Judge> {
        let base = self.offsets.judge_data;

        let p1 = PlayerJudge {
            pgreat: reader.read_u32(base + judge::P1_PGREAT)?,
            great: reader.read_u32(base + judge::P1_GREAT)?,
            good: reader.read_u32(base + judge::P1_GOOD)?,
            bad: reader.read_u32(base + judge::P1_BAD)?,
            poor: reader.read_u32(base + judge::P1_POOR)?,
            combo_break: reader.read_u32(base + judge::P1_COMBO_BREAK)?,
            fast: reader.read_u32(base + judge::P1_FAST)?,
            slow: reader.read_u32(base + judge::P1_SLOW)?,
            measure_end: reader.read_u32(base + judge::P1_MEASURE_END)?,
        };

        let p2 = PlayerJudge {
            pgreat: reader.read_u32(base + judge::P2_PGREAT)?,
            great: reader.read_u32(base + judge::P2_GREAT)?,
            good: reader.read_u32(base + judge::P2_GOOD)?,
            bad: reader.read_u32(base + judge::P2_BAD)?,
            poor: reader.read_u32(base + judge::P2_POOR)?,
            combo_break: reader.read_u32(base + judge::P2_COMBO_BREAK)?,
            fast: reader.read_u32(base + judge::P2_FAST)?,
            slow: reader.read_u32(base + judge::P2_SLOW)?,
            measure_end: reader.read_u32(base + judge::P2_MEASURE_END)?,
        };

        Ok(Judge::from_raw_data(RawJudgeData { p1, p2 }))
    }

    fn fetch_settings(&self, reader: &MemoryReader, play_type: PlayType) -> Result<Settings> {
        let word: u64 = 4;
        let base = self.offsets.play_settings;

        let (style_val, assist_val, range_val, h_ran_val, style2_val) = match play_type {
            PlayType::P1 | PlayType::Dp => {
                let style = reader.read_i32(base)?;
                let assist = reader.read_i32(base + word * 2)?;
                let range = reader.read_i32(base + word * 4)?;
                let h_ran = reader.read_i32(base + word * 9)?;
                let style2 = if play_type == PlayType::Dp {
                    reader.read_i32(base + word * 5)?
                } else {
                    0
                };
                (style, assist, range, h_ran, style2)
            }
            PlayType::P2 => {
                let p2_offset = Settings::P2_OFFSET;
                let style = reader.read_i32(base + p2_offset)?;
                let assist = reader.read_i32(base + p2_offset + word * 2)?;
                let range = reader.read_i32(base + p2_offset + word * 4)?;
                let h_ran = reader.read_i32(base + p2_offset + word * 9)?;
                (style, assist, range, h_ran, 0)
            }
        };

        let flip_val = reader.read_i32(base + word * 3)?;
        let battle_val = reader.read_i32(base + word * 8)?;

        Ok(Settings::from_raw_values(
            play_type, style_val, style2_val, assist_val, range_val, flip_val, battle_val,
            h_ran_val,
        ))
    }

    /// Load current unlock state from memory
    pub fn load_unlock_state(&mut self, reader: &MemoryReader) -> Result<()> {
        if self.game_data.song_db.is_empty() {
            warn!("Song database is empty, cannot load unlock state");
            return Ok(());
        }

        self.game_data.unlock_state =
            get_unlock_states(reader, self.offsets.unlock_data, &self.game_data.song_db)?;
        debug!(
            "Loaded unlock state from memory ({} entries)",
            self.game_data.unlock_state.len()
        );
        Ok(())
    }

    /// Check game version and compare with offsets version
    ///
    /// Returns (game_version, matches) where matches is true if versions match
    pub fn check_game_version(
        &self,
        reader: &MemoryReader,
        base_address: u64,
    ) -> Result<(Option<String>, bool)> {
        let game_version = find_game_version(reader, base_address)?;

        let matches = match &game_version {
            Some(version) => check_version_match(version, &self.offsets.version),
            None => false,
        };

        Ok((game_version, matches))
    }
}
