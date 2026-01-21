//! Game loop and state handling for Reflux
//!
//! This module contains the main tracking loop and game state handling methods.

use std::thread;
use std::time::Duration;

use chrono::Utc;
use tracing::{debug, error, info, warn};

// =============================================================================
// Retry and polling configuration
// =============================================================================

/// Memory read retry settings.
/// Exponential backoff: 50ms → 100ms → 200ms = total 350ms max.
/// Handles transient read failures while keeping latency acceptable.
const MAX_READ_RETRIES: u32 = 3;
const RETRY_DELAYS_MS: [u64; 3] = [50, 100, 200];

/// Result screen polling delays (exponential backoff).
/// Total: 50+50+100+100+200+200+300+300+500+500 = 2.3 seconds max.
/// Faster initial polling catches quick data availability, while exponential
/// backoff reduces CPU usage if data takes longer to populate.
const POLL_DELAYS_MS: [u64; 10] = [50, 50, 100, 100, 200, 200, 300, 300, 500, 500];

use crate::error::Result;
use crate::game::{
    AssistType, ChartInfo, Difficulty, GameState, Grade, Judge, Lamp, PlayData, PlayType,
    PlayerJudge, RawJudgeData, Settings, check_version_match, find_game_version, get_unlock_states,
};
use crate::memory::layout::{judge, play, settings, timing};
use crate::memory::{MemoryReader, ProcessHandle, ReadMemory};
use crate::storage::{ChartKey, TrackerInfo, format_play_data_console};

use super::Reflux;

impl Reflux {
    /// Run the main tracking loop
    pub fn run(&mut self, process: &ProcessHandle) -> Result<()> {
        let reader = MemoryReader::new(process);
        let mut last_state = GameState::Unknown;

        info!("Starting tracker loop...");

        // Start TSV session
        self.session_manager = crate::storage::SessionManager::new("sessions");
        match self.session_manager.start_tsv_session() {
            Ok(path) => info!("Started TSV session at {:?}", path),
            Err(e) => warn!("Failed to start TSV session: {}", e),
        }

        loop {
            // Check if process is still alive with retry mechanism (exponential backoff)
            let mut process_alive = false;
            for attempt in 0..MAX_READ_RETRIES {
                match reader.read_bytes(process.base_address, 4) {
                    Ok(_) => {
                        process_alive = true;
                        break;
                    }
                    Err(e) => {
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
                            info!(
                                "Process terminated after {} retries: {}",
                                MAX_READ_RETRIES, e
                            );
                        }
                    }
                }
            }
            if !process_alive {
                break;
            }

            // Detect game state
            let current_state = self.detect_game_state(&reader)?;

            if current_state != last_state {
                info!("State changed: {:?} -> {:?}", last_state, current_state);
                self.handle_state_change(&reader, last_state, current_state)?;
                last_state = current_state;
            }

            thread::sleep(Duration::from_millis(timing::GAME_STATE_POLL_INTERVAL_MS));
        }

        Ok(())
    }

    fn detect_game_state(&mut self, reader: &MemoryReader) -> Result<GameState> {
        // Read markers for state detection
        let state_marker_1 = reader
            .read_i32(self.offsets.judge_data + judge::STATE_MARKER_1)
            .unwrap_or(0);
        let state_marker_2 = reader
            .read_i32(self.offsets.judge_data + judge::STATE_MARKER_2)
            .unwrap_or(0);
        let song_select_marker = reader
            .read_i32(
                self.offsets
                    .play_settings
                    .wrapping_sub(settings::SONG_SELECT_MARKER),
            )
            .unwrap_or(0);

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
                    if total_notes > 0 {
                        self.process_play_result(&play_data);
                        return;
                    }
                    // Data not ready yet, continue polling
                    if attempt == POLL_DELAYS_MS.len() - 1 {
                        warn!(
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

        // Update tracker
        self.update_tracker(play_data);

        // Save to session files
        self.save_session_data(play_data);
    }

    /// Save play data to session file (TSV)
    fn save_session_data(&mut self, play_data: &PlayData) {
        if let Err(e) = self.session_manager.append_tsv_row(play_data) {
            error!("Failed to append TSV row: {}", e);
        }
    }

    /// Handle transition to song select screen
    fn handle_song_select(&mut self, reader: &MemoryReader) {
        // Poll unlock state changes
        self.poll_unlock_changes(reader);

        // Export tracker.tsv
        if let Err(e) = self.export_tracker_tsv("tracker.tsv") {
            error!("Failed to export tracker.tsv: {}", e);
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
            info!("Detected {} unlock state changes", changes.len());

            // Update local state
            for (&song_id, unlock_data) in &changes {
                self.unlock_db.update_from_data(song_id, unlock_data);
            }
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

    fn fetch_play_data(&self, reader: &MemoryReader) -> Result<PlayData> {
        // Read basic play data
        let song_id = reader.read_i32(self.offsets.play_data + play::SONG_ID)? as u32;
        let difficulty_val = reader.read_i32(self.offsets.play_data + play::DIFFICULTY)?;
        let lamp_val = reader.read_i32(self.offsets.play_data + play::LAMP)?;

        let difficulty = Difficulty::from_u8(difficulty_val as u8).unwrap_or(Difficulty::SpN);
        let mut lamp = Lamp::from_u8(lamp_val as u8).unwrap_or(Lamp::NoPlay);

        // Fetch judge data
        let judge = self.fetch_judge_data(reader)?;

        // Upgrade to PFC if applicable
        if judge.is_pfc() && lamp == Lamp::FullCombo {
            lamp = Lamp::Pfc;
        }

        // Calculate EX score
        let ex_score = judge.ex_score();

        // Fetch settings
        let settings = self.fetch_settings(reader, judge.play_type)?;
        let data_available =
            !settings.h_ran && !settings.battle && settings.assist == AssistType::Off;

        let chart = self.create_chart_info(song_id, difficulty);

        // Calculate grade
        let grade = if chart.total_notes > 0 {
            PlayData::calculate_grade(ex_score, chart.total_notes)
        } else {
            Grade::NoPlay
        };

        let gauge = self.read_gauge(reader);

        Ok(PlayData {
            timestamp: Utc::now(),
            chart,
            ex_score,
            gauge,
            grade,
            lamp,
            judge,
            settings,
            data_available,
        })
    }

    /// Create chart info from song database or generate a placeholder
    fn create_chart_info(&self, song_id: u32, difficulty: Difficulty) -> ChartInfo {
        if let Some(song) = self.game_data.song_db.get(&song_id) {
            ChartInfo::from_song_info(song, difficulty, true)
        } else {
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
    }

    /// Read gauge value from memory (P1 + P2 combined)
    fn read_gauge(&self, reader: &MemoryReader) -> u8 {
        let gauge_p1 = reader
            .read_i32(self.offsets.judge_data + judge::P1_GAUGE)
            .unwrap_or(0);
        let gauge_p2 = reader
            .read_i32(self.offsets.judge_data + judge::P2_GAUGE)
            .unwrap_or(0);
        (gauge_p1 + gauge_p2) as u8
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

        let (style_val, gauge_val, assist_val, range_val, h_ran_val, style2_val) = match play_type {
            PlayType::P1 | PlayType::Dp => {
                let style = reader.read_i32(base)?;
                let gauge = reader.read_i32(base + word)?;
                let assist = reader.read_i32(base + word * 2)?;
                let range = reader.read_i32(base + word * 4)?;
                let h_ran = reader.read_i32(base + word * 9)?;
                let style2 = if play_type == PlayType::Dp {
                    reader.read_i32(base + word * 5)?
                } else {
                    0
                };
                (style, gauge, assist, range, h_ran, style2)
            }
            PlayType::P2 => {
                let p2_offset = Settings::P2_OFFSET;
                let style = reader.read_i32(base + p2_offset)?;
                let gauge = reader.read_i32(base + p2_offset + word)?;
                let assist = reader.read_i32(base + p2_offset + word * 2)?;
                let range = reader.read_i32(base + p2_offset + word * 4)?;
                let h_ran = reader.read_i32(base + p2_offset + word * 9)?;
                (style, gauge, assist, range, h_ran, 0)
            }
        };

        let flip_val = reader.read_i32(base + word * 3)?;
        let battle_val = reader.read_i32(base + word * 8)?;

        Ok(Settings::from_raw_values(
            play_type, style_val, style2_val, gauge_val, assist_val, range_val, flip_val,
            battle_val, h_ran_val,
        ))
    }

    fn update_tracker(&mut self, play_data: &PlayData) {
        let key = ChartKey {
            song_id: play_data.chart.song_id,
            difficulty: play_data.chart.difficulty,
        };

        let new_info = TrackerInfo {
            grade: play_data.grade,
            lamp: play_data.lamp,
            ex_score: play_data.ex_score,
            miss_count: if play_data.miss_count_valid() {
                Some(play_data.miss_count())
            } else {
                None
            },
            dj_points: crate::game::calculate_dj_points(
                play_data.ex_score,
                play_data.grade,
                play_data.lamp,
            ),
        };

        self.tracker.update(key, new_info);
    }

    /// Load current unlock state from memory
    pub fn load_unlock_state(&mut self, reader: &MemoryReader) -> Result<()> {
        if self.game_data.song_db.is_empty() {
            warn!("Song database is empty, cannot load unlock state");
            return Ok(());
        }

        self.game_data.unlock_state =
            get_unlock_states(reader, self.offsets.unlock_data, &self.game_data.song_db)?;
        info!(
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
