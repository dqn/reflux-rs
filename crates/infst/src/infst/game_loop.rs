//! Game loop and state handling for INFST
//!
//! This module contains the main tracking loop and game state handling methods.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use tracing::{debug, error, info, warn};

use crate::chart::{
    ChartInfo, Difficulty, fetch_song_by_id, fetch_song_database_from_memory_scan,
    get_unlock_states,
};
use crate::config::{check_version_match, find_game_version, polling, retry};
use crate::error::Result;
use crate::export::format_play_data_console;
use crate::play::{AssistType, GameState, PlayData, PlayType, RawSettings, Settings};
use crate::process::layout::{judge, play, settings, timing};
use crate::process::{MemoryReader, ProcessHandle, ReadMemory};
use crate::score::{Grade, Judge, Lamp, PlayerJudge, RawJudgeData, ScoreMap};

use super::Infst;

/// Read a value from memory with a default on error.
///
/// This helper simplifies error handling for non-critical reads.
fn read_with_default<T, F>(f: F, default: T, context: &str) -> T
where
    F: FnOnce() -> Result<T>,
{
    match f() {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to read {}: {}", context, e);
            default
        }
    }
}

/// Check if memory is accessible with retry logic.
///
/// Uses exponential backoff and checks process liveness between retries.
fn verify_memory_access(reader: &MemoryReader, process: &ProcessHandle) -> bool {
    for attempt in 0..retry::MAX_READ_RETRIES {
        match reader.read_bytes(process.base_address, 4) {
            Ok(_) => return true,
            Err(e) => {
                // Re-check process status before retrying
                if !process.is_alive() {
                    debug!("Process terminated during retry: {}", e);
                    return false;
                }

                if attempt < retry::MAX_READ_RETRIES - 1 {
                    let delay = retry::RETRY_DELAYS_MS[attempt as usize];
                    debug!(
                        "Memory read failed (attempt {}/{}, retry in {}ms): {}",
                        attempt + 1,
                        retry::MAX_READ_RETRIES,
                        delay,
                        e
                    );
                    thread::sleep(Duration::from_millis(delay));
                } else {
                    debug!(
                        "Memory read failed after {} retries: {}",
                        retry::MAX_READ_RETRIES,
                        e
                    );
                }
            }
        }
    }
    false
}

impl Infst {
    /// Run the main tracking loop
    ///
    /// The `shutdown_requested` flag is checked each iteration to allow graceful shutdown via Ctrl+C.
    /// When `shutdown_requested` is `true`, the loop exits.
    pub fn run(&mut self, process: &ProcessHandle, shutdown_requested: &AtomicBool) -> Result<()> {
        let reader = MemoryReader::new(process);
        let mut last_state = GameState::Unknown;

        debug!("Starting tracker loop...");

        // Start TSV session
        self.session_manager = crate::session::SessionManager::new("sessions");
        match self.session_manager.start_tsv_session() {
            Ok(path) => debug!("Started TSV session at {:?}", path),
            Err(e) => warn!("Failed to start TSV session: {}", e),
        }

        loop {
            // Check for shutdown signal
            if shutdown_requested.load(Ordering::SeqCst) {
                debug!("Shutdown signal received, exiting tracker loop");
                break;
            }

            // Step 1: Fast check if process is still alive via exit code
            if !process.is_alive() {
                debug!("Process terminated (exit code check)");
                break;
            }

            // Step 2: Verify memory access with retry mechanism (exponential backoff)
            if !verify_memory_access(&reader, process) {
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
        let state_marker_1 = read_with_default(
            || reader.read_i32(self.offsets.judge_data + judge::STATE_MARKER_1),
            0,
            "state_marker_1",
        );
        let state_marker_2 = read_with_default(
            || reader.read_i32(self.offsets.judge_data + judge::STATE_MARKER_2),
            0,
            "state_marker_2",
        );
        let song_select_marker = read_with_default(
            || {
                reader.read_i32(
                    self.offsets
                        .play_settings
                        .wrapping_sub(settings::SONG_SELECT_MARKER),
                )
            },
            0,
            "song_select_marker",
        );

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
        for (attempt, &delay) in polling::POLL_DELAYS_MS.iter().enumerate() {
            thread::sleep(Duration::from_millis(delay));

            match self.fetch_play_data(reader) {
                Ok(play_data) => {
                    // Verify data looks valid (non-zero total notes)
                    let total_notes = play_data.judge.pgreat
                        + play_data.judge.great
                        + play_data.judge.good
                        + play_data.judge.bad
                        + play_data.judge.poor;

                    // Validate song_id matches current_playing (if available)
                    let song_id_valid = match self.current_playing {
                        Some((expected_id, _)) => play_data.chart.song_id == expected_id,
                        None => true, // No reference, accept any
                    };

                    debug!(
                        "Attempt {}: song_id={}, total_notes={}, song_id_valid={}, judge: P={} G={} Go={} B={} Po={}",
                        attempt + 1,
                        play_data.chart.song_id,
                        total_notes,
                        song_id_valid,
                        play_data.judge.pgreat,
                        play_data.judge.great,
                        play_data.judge.good,
                        play_data.judge.bad,
                        play_data.judge.poor
                    );

                    if total_notes > 0 && song_id_valid {
                        info!(
                            "Play result captured: {} ({}) - EX: {}",
                            play_data.chart.title, play_data.chart.song_id, play_data.ex_score
                        );
                        self.process_play_result(&play_data);
                        self.current_playing = None; // Clear after processing
                        return;
                    }
                    // Data not ready yet, continue polling
                    if attempt == polling::POLL_DELAYS_MS.len() - 1 {
                        debug!(
                            "Play data notes count is zero or song_id mismatch after {} attempts",
                            polling::POLL_DELAYS_MS.len()
                        );
                    }
                }
                Err(e) => {
                    if attempt == polling::POLL_DELAYS_MS.len() - 1 {
                        error!(
                            "Failed to fetch play data after {} attempts: {}",
                            polling::POLL_DELAYS_MS.len(),
                            e
                        );
                    }
                }
            }
        }

        // Clear current_playing even if we failed to capture data
        self.current_playing = None;
    }

    /// Process and save play result data
    fn process_play_result(&mut self, play_data: &PlayData) {
        // Get personal best for comparison
        let personal_best = self.game_data.score_map.get(play_data.chart.song_id);

        // Print detailed play data to console (with PB comparison)
        println!("{}", format_play_data_console(play_data, personal_best));

        // Save to session files
        self.save_session_data(play_data);

        // Send to API (non-blocking)
        self.send_lamp_to_api(play_data);
    }

    /// Send lamp data to the API endpoint in a background thread
    #[cfg(feature = "api")]
    fn send_lamp_to_api(&self, play_data: &PlayData) {
        let Some(ref api_config) = self.config.api_config else {
            return;
        };

        // Only level 11/12 charts are synced to the web API.
        if !matches!(play_data.chart.level, 11 | 12) {
            return;
        }

        let endpoint = api_config.endpoint.clone();
        let token = api_config.token.clone();
        let song_id = play_data.chart.song_id;
        let difficulty = play_data.chart.difficulty.short_name().to_string();
        let lamp = play_data.lamp.short_name().to_string();
        let ex_score = play_data.ex_score;
        let miss_count = play_data.miss_count();

        thread::spawn(move || {
            if let Err(e) = send_lamp_request(
                &endpoint,
                &token,
                song_id,
                &difficulty,
                &lamp,
                ex_score,
                miss_count,
            ) {
                warn!("Failed to send lamp to API: {}", e);
            }
        });
    }

    #[cfg(not(feature = "api"))]
    fn send_lamp_to_api(&self, _play_data: &PlayData) {}

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
        let prev_count = self.game_data.song_db.len();
        self.rescan_song_database(reader);

        // Poll unlock state changes
        self.poll_unlock_changes(reader);

        // Reload score map if new songs were discovered
        if self.game_data.song_db.len() > prev_count {
            self.reload_score_map(reader);
        }

        // Export tracker file if auto-export is enabled
        if self.config.auto_export
            && let Err(e) = self.export_tracker_tsv(&self.config.tracker_path)
        {
            error!("Failed to export tracker file: {}", e);
        }
    }

    /// Reload score map from memory
    ///
    /// Called when new songs are discovered to ensure score comparisons
    /// work for all known songs.
    fn reload_score_map(&mut self, reader: &MemoryReader) {
        match ScoreMap::load_from_memory(reader, self.offsets.data_map, &self.game_data.song_db) {
            Ok(map) => {
                info!("Reloaded score map: {} entries", map.len());
                self.game_data.score_map = map;
            }
            Err(e) => warn!("Failed to reload score map: {}", e),
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
            if let std::collections::hash_map::Entry::Vacant(e) =
                self.game_data.song_db.entry(song_id)
            {
                debug!(
                    "Discovered new song via rescan: {} ({})",
                    song.title, song_id
                );
                e.insert(song);
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
    ///
    /// Captures current chart selection when entering Playing state.
    /// This is used for cross-validation on ResultScreen to ensure
    /// we're reading the correct play data.
    fn handle_playing(&mut self, reader: &MemoryReader) {
        match self.fetch_current_chart(reader) {
            Ok((song_id, difficulty)) => {
                debug!(
                    "Entering Playing state: song_id={}, difficulty={:?}",
                    song_id, difficulty
                );
                self.current_playing = Some((song_id, difficulty));
            }
            Err(e) => {
                warn!("Failed to fetch current chart on Playing: {}", e);
                // Keep previous value if any, or None
            }
        }
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
            crate::chart::detect_unlock_changes(&self.game_data.unlock_state, &current_state);

        if !changes.is_empty() {
            debug!("Detected {} unlock state changes", changes.len());
        }

        // Update current unlock state
        self.game_data.unlock_state = current_state;
    }

    /// Fetch current chart selection from memory
    ///
    /// Used during Playing state to capture what chart is being played,
    /// enabling cross-validation when reading play data on ResultScreen.
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
        let lamp = Lamp::from_u8(lamp_val as u8).unwrap_or(Lamp::NoPlay);

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
            info!("Dynamically loaded song: {} ({})", song.title, song_id);
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

        let (style, assist, range, h_ran, style2) = match play_type {
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

        let flip = reader.read_i32(base + word * 3)?;
        let battle = reader.read_i32(base + word * 8)?;

        Ok(Settings::from_raw(RawSettings {
            play_type,
            style,
            style2,
            assist,
            range,
            flip,
            battle,
            h_ran,
        }))
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

#[cfg(feature = "api")]
fn send_lamp_request(
    endpoint: &str,
    token: &str,
    song_id: u32,
    difficulty: &str,
    lamp: &str,
    ex_score: u32,
    miss_count: u32,
) -> anyhow::Result<()> {
    let url = format!("{}/api/lamps", endpoint.trim_end_matches('/'));
    let body = serde_json::json!({
        "songId": song_id,
        "difficulty": difficulty,
        "lamp": lamp,
        "exScore": ex_score,
        "missCount": miss_count,
    });

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(5)))
        .build();
    let agent: ureq::Agent = config.into();
    let response = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .send_json(&body)?;

    tracing::debug!("API response: {}", response.status());
    Ok(())
}
