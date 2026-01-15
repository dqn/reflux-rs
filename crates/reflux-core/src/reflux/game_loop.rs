//! Game loop and state handling for Reflux
//!
//! This module contains the main tracking loop and game state handling methods.

use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use tracing::{debug, error, info, warn};

use crate::error::Result;
use crate::game::{
    check_version_match, find_game_version, get_unlock_state_for_difficulty, get_unlock_states,
    AssistType, ChartInfo, Difficulty, GameState, Grade, Judge, Lamp, PlayData, PlayType, Settings,
    SongInfo, UnlockType,
};
use crate::memory::layout::{judge, play, settings, timing};
use crate::memory::{MemoryReader, ProcessHandle, ReadMemory};
use crate::network::{AddSongParams, RefluxApi};
use crate::storage::{
    export_tracker_tsv, format_play_data_console, format_post_form, ChartKey, TrackerInfo,
};

use super::{Reflux, UpdateResult};

impl Reflux {
    /// Run the main tracking loop
    pub fn run(&mut self, process: &ProcessHandle) -> Result<()> {
        let reader = MemoryReader::new(process);
        let mut last_state = GameState::Unknown;

        info!("Starting tracker loop...");

        if self.config.record.save_local || self.config.record.save_json {
            self.session_manager = crate::storage::SessionManager::new("sessions");

            if self.config.record.save_local {
                match self
                    .session_manager
                    .start_session_with_header(&self.config.local_record)
                {
                    Ok(path) => info!("Started TSV session at {:?}", path),
                    Err(e) => warn!("Failed to start TSV session: {}", e),
                }
            }

            if self.config.record.save_json {
                match self.session_manager.start_json_session() {
                    Ok(path) => info!("Started JSON session at {:?}", path),
                    Err(e) => warn!("Failed to start JSON session: {}", e),
                }
            }
        }

        // Initialize streaming files
        if self.config.livestream.show_play_state {
            let _ = self.stream_output.write_play_state(GameState::SongSelect);
        }
        if self.config.livestream.enable_marquee {
            let _ = self
                .stream_output
                .write_marquee(&self.config.livestream.marquee_idle_text);
        }

        const MAX_READ_RETRIES: u32 = 3;
        const RETRY_DELAYS_MS: [u64; 3] = [50, 100, 200];

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

        // Cleanup
        if self.config.livestream.show_play_state {
            let _ = self.stream_output.write_play_state(GameState::Unknown);
        }
        if self.config.livestream.enable_marquee {
            let _ = self.stream_output.write_marquee("NO SIGNAL");
        }

        // Report failed remote API calls if any
        let failed_count = self.api_error_tracker.count();
        if failed_count > 0 {
            warn!(
                "{} remote API call(s) failed during this session",
                failed_count
            );
            // Log summary by endpoint
            for (endpoint, count) in self.api_error_tracker.summary() {
                warn!("  - {}: {} failure(s)", endpoint, count);
            }
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
        const MAX_POLL_ATTEMPTS: u32 = 50;
        const POLL_INTERVAL_MS: u64 = 100;

        // Poll until play data becomes available (max 5 seconds)
        for attempt in 0..MAX_POLL_ATTEMPTS {
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

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
                    if attempt == MAX_POLL_ATTEMPTS - 1 {
                        warn!(
                            "Play data notes count is zero after {} attempts",
                            MAX_POLL_ATTEMPTS
                        );
                    }
                }
                Err(e) => {
                    if attempt == MAX_POLL_ATTEMPTS - 1 {
                        error!(
                            "Failed to fetch play data after {} attempts: {}",
                            MAX_POLL_ATTEMPTS, e
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

        // Send to remote server
        self.send_to_remote(play_data);

        // Write latest files for OBS/streaming
        self.write_latest_files(play_data);

        // Update streaming files
        self.update_result_stream_files(play_data);
    }

    /// Save play data to session files (TSV and JSON)
    fn save_session_data(&mut self, play_data: &PlayData) {
        if self.config.record.save_local
            && let Err(e) = self
                .session_manager
                .append_tsv_row(play_data, &self.config.local_record)
        {
            error!("Failed to append TSV row: {}", e);
        }

        if self.config.record.save_json
            && let Err(e) = self.session_manager.append_json_entry(play_data)
        {
            error!("Failed to append JSON entry: {}", e);
        }
    }

    /// Send play data to remote server
    fn send_to_remote(&self, play_data: &PlayData) {
        if self.config.record.save_remote
            && let Some(api) = self.api.clone()
            && let Some(handle) = &self.runtime_handle
        {
            let form = format_post_form(play_data, &self.config.remote_record.api_key);
            let error_tracker = Arc::clone(&self.api_error_tracker);

            // Capture payload summary for error logging
            let payload_summary = format!(
                "song_id={}, title={}, diff={}, ex_score={}",
                play_data.chart.song_id,
                play_data.chart.title,
                play_data.chart.difficulty.short_name(),
                play_data.ex_score
            );

            handle.spawn(async move {
                if let Err(e) = api.report_play(form).await {
                    error_tracker.record("report_play", e.to_string(), &payload_summary);
                    tracing::error!(
                        "Failed to report play to remote: {} (payload: {})",
                        e,
                        payload_summary
                    );
                }
            });
        }
    }

    /// Write latest play files for OBS
    fn write_latest_files(&mut self, play_data: &PlayData) {
        let write_json = self.config.record.save_latest_json;
        let write_txt = self.config.record.save_latest_txt;

        if (write_json || write_txt)
            && let Err(e) = self.stream_output.write_latest_files(
                play_data,
                &self.config.remote_record.api_key,
                write_json,
                write_txt,
            )
        {
            error!("Failed to write latest files: {}", e);
        }
    }

    /// Update streaming files for result screen
    fn update_result_stream_files(&mut self, play_data: &PlayData) {
        if self.config.livestream.show_play_state {
            let _ = self.stream_output.write_play_state(GameState::ResultScreen);
        }

        if self.config.livestream.enable_marquee {
            let status = if play_data.lamp == Lamp::Failed {
                "FAIL!"
            } else {
                "CLEAR!"
            };
            let _ = self
                .stream_output
                .write_marquee(&format!("{} {}", play_data.chart.title_english, status));
        }
    }

    /// Handle transition to song select screen
    fn handle_song_select(&mut self, reader: &MemoryReader) {
        // Update streaming files
        if self.config.livestream.show_play_state {
            let _ = self.stream_output.write_play_state(GameState::SongSelect);
        }
        if self.config.livestream.enable_marquee {
            let _ = self
                .stream_output
                .write_marquee(&self.config.livestream.marquee_idle_text);
        }

        // Clear full song info files
        if self.config.livestream.enable_full_song_info {
            let _ = self.stream_output.clear_full_song_info();
        }

        // Poll unlock state changes and report to server
        self.poll_unlock_changes(reader);

        // Export tracker.tsv if save_local is enabled
        if self.config.record.save_local
            && let Err(e) = export_tracker_tsv(
                "tracker.tsv",
                &self.tracker,
                &self.game_data.song_db,
                &self.game_data.unlock_state,
                &self.game_data.score_map,
                &self.game_data.custom_types,
            )
        {
            error!("Failed to export tracker.tsv: {}", e);
        }
    }

    /// Handle transition to playing state
    fn handle_playing(&mut self, reader: &MemoryReader) {
        if let Ok((song_id, difficulty)) = self.fetch_current_chart(reader)
            && let Some(song) = self.game_data.song_db.get(&song_id)
        {
            let chart_name = format!("{} {}", song.title_english, difficulty.short_name());

            if self.config.livestream.show_play_state {
                let _ = self.stream_output.write_play_state(GameState::Playing);
            }
            if self.config.livestream.enable_marquee {
                let _ = self.stream_output.write_marquee(&chart_name.to_uppercase());
            }

            // Write full song info files for OBS
            if self.config.livestream.enable_full_song_info
                && let Err(e) = self.stream_output.write_full_song_info(song, difficulty)
            {
                error!("Failed to write full song info: {}", e);
            }
        }
    }

    /// Poll for unlock state changes and report to server
    fn poll_unlock_changes(&mut self, reader: &MemoryReader) {
        if !self.config.record.save_remote || self.api.is_none() {
            return;
        }

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

            // Report changes to server
            for (&song_id, unlock_data) in &changes {
                if let Some(api) = &self.api
                    && let Some(handle) = &self.runtime_handle
                {
                    let api_clone = api.clone();
                    let error_tracker = Arc::clone(&self.api_error_tracker);
                    let song_id_str = format!("{:05}", song_id);
                    let unlocks = unlock_data.unlocks;
                    handle.spawn(async move {
                        if let Err(e) = api_clone.report_unlock(&song_id_str, unlocks).await {
                            error_tracker.record("report_unlock", e.to_string(), &song_id_str);
                            tracing::error!("Failed to report unlock for {}: {}", song_id_str, e);
                        }
                    });
                }

                // Update local state
                self.unlock_db.update_from_data(song_id, unlock_data);
            }
        }

        // Update current unlock state
        self.game_data.unlock_state = current_state;
    }

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

        // Get or create chart info
        let chart = if let Some(song) = self.game_data.song_db.get(&song_id) {
            ChartInfo::from_song_info(song, difficulty, true)
        } else {
            // Create minimal chart info
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
        };

        // Calculate grade
        let grade = if chart.total_notes > 0 {
            PlayData::calculate_grade(ex_score, chart.total_notes)
        } else {
            Grade::NoPlay
        };

        // Read gauge (P1 + P2 combined)
        let gauge_p1 = reader
            .read_i32(self.offsets.judge_data + judge::P1_GAUGE)
            .unwrap_or(0);
        let gauge_p2 = reader
            .read_i32(self.offsets.judge_data + judge::P2_GAUGE)
            .unwrap_or(0);
        let gauge = (gauge_p1 + gauge_p2) as u8;

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

    fn fetch_judge_data(&self, reader: &MemoryReader) -> Result<Judge> {
        let base = self.offsets.judge_data;

        // Player 1 judge counts
        let p1_pgreat = reader.read_u32(base + judge::P1_PGREAT)?;
        let p1_great = reader.read_u32(base + judge::P1_GREAT)?;
        let p1_good = reader.read_u32(base + judge::P1_GOOD)?;
        let p1_bad = reader.read_u32(base + judge::P1_BAD)?;
        let p1_poor = reader.read_u32(base + judge::P1_POOR)?;

        // Player 2 judge counts
        let p2_pgreat = reader.read_u32(base + judge::P2_PGREAT)?;
        let p2_great = reader.read_u32(base + judge::P2_GREAT)?;
        let p2_good = reader.read_u32(base + judge::P2_GOOD)?;
        let p2_bad = reader.read_u32(base + judge::P2_BAD)?;
        let p2_poor = reader.read_u32(base + judge::P2_POOR)?;

        // Combo break counts
        let p1_cb = reader.read_u32(base + judge::P1_COMBO_BREAK)?;
        let p2_cb = reader.read_u32(base + judge::P2_COMBO_BREAK)?;

        // Fast/Slow counts
        let p1_fast = reader.read_u32(base + judge::P1_FAST)?;
        let p2_fast = reader.read_u32(base + judge::P2_FAST)?;
        let p1_slow = reader.read_u32(base + judge::P1_SLOW)?;
        let p2_slow = reader.read_u32(base + judge::P2_SLOW)?;

        // Measure end markers (for premature end detection)
        let p1_measure_end = reader.read_u32(base + judge::P1_MEASURE_END)?;
        let p2_measure_end = reader.read_u32(base + judge::P2_MEASURE_END)?;

        Ok(Judge::from_raw_values(
            p1_pgreat,
            p1_great,
            p1_good,
            p1_bad,
            p1_poor,
            p2_pgreat,
            p2_great,
            p2_good,
            p2_bad,
            p2_poor,
            p1_cb,
            p2_cb,
            p1_fast,
            p2_fast,
            p1_slow,
            p2_slow,
            p1_measure_end,
            p2_measure_end,
        ))
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

    /// Sync with remote server
    ///
    /// This function:
    /// 1. Compares song_db with unlock_db to find new songs
    /// 2. Uploads new songs and their charts
    /// 3. Reports unlock type changes
    /// 4. Reports unlock state changes
    pub async fn sync_with_server(&mut self) -> Result<()> {
        let api = match &self.api {
            Some(api) => api,
            None => {
                info!("Remote saving disabled, skipping server sync");
                return Ok(());
            }
        };

        info!("Checking for songs/charts to update at remote...");

        // Count new songs (not in unlock_db)
        let new_songs: Vec<_> = self
            .game_data
            .song_db
            .keys()
            .filter(|id| !self.unlock_db.contains(**id))
            .copied()
            .collect();

        if !new_songs.is_empty() {
            info!("Found {} songs to upload to remote", new_songs.len());
        }

        for (i, &song_id) in self.game_data.song_db.keys().enumerate() {
            let Some(song) = self.game_data.song_db.get(&song_id) else {
                continue;
            };

            // Report progress for new songs
            if !new_songs.is_empty() && (i % 100 == 0 || i == self.game_data.song_db.len() - 1) {
                let percent = (i * 100) / self.game_data.song_db.len();
                info!("Sync progress: {}%", percent);
            }

            // Upload new songs
            if !self.unlock_db.contains(song_id) {
                self.upload_song_info(api, song_id, song).await?;
            }

            // Check for unlock type/state changes
            if let Some(unlock_data) = self.game_data.unlock_state.get(&song_id) {
                let current_type = match unlock_data.unlock_type {
                    UnlockType::Base => 1,
                    UnlockType::Bits => 2,
                    UnlockType::Sub => 3,
                };

                // Check unlock type change
                if self
                    .unlock_db
                    .has_unlock_type_changed(song_id, current_type)
                {
                    info!("Unlock type changed for {:05}: updating remote", song_id);
                    let song_id_str = format!("{:05}", song_id);
                    if let Err(e) = api
                        .update_chart_unlock_type(&song_id_str, current_type as u8)
                        .await
                    {
                        error!("Failed to update unlock type for {:05}: {}", song_id, e);
                    }
                }

                // Check unlock state change
                if self
                    .unlock_db
                    .has_unlocks_changed(song_id, unlock_data.unlocks)
                {
                    info!(
                        "Unlock state changed for {:05}: reporting to remote",
                        song_id
                    );
                    let song_id_str = format!("{:05}", song_id);
                    if let Err(e) = api.report_unlock(&song_id_str, unlock_data.unlocks).await {
                        error!("Failed to report unlock for {:05}: {}", song_id, e);
                    }
                }

                // Update local unlock_db
                self.unlock_db.update_from_data(song_id, unlock_data);
            }
        }

        info!("Server sync completed");
        Ok(())
    }

    /// Upload song info and charts to remote server
    async fn upload_song_info(&self, api: &RefluxApi, song_id: u32, song: &SongInfo) -> Result<()> {
        let song_id_str = format!("{:05}", song_id);
        let unlock_type = self
            .game_data
            .unlock_state
            .get(&song_id)
            .map(|u| match u.unlock_type {
                UnlockType::Base => 1,
                UnlockType::Bits => 2,
                UnlockType::Sub => 3,
            })
            .unwrap_or(1);

        // Add song
        let params = AddSongParams {
            song_id: &song_id_str,
            title: &song.title,
            title_english: &song.title_english,
            artist: &song.artist,
            genre: &song.genre,
            bpm: &song.bpm,
            unlock_type,
        };

        if let Err(e) = api.add_song(params).await {
            error!("Failed to add song {:05}: {}", song_id, e);
            return Ok(());
        }

        // Add charts for each difficulty (skip SPB=0 and DPB=5)
        for diff_idx in [1, 2, 3, 4, 6, 7, 8, 9] {
            let level = song.levels.get(diff_idx).copied().unwrap_or(0);
            if level == 0 {
                continue;
            }

            let difficulty = match Difficulty::from_u8(diff_idx as u8) {
                Some(d) => d,
                None => continue,
            };

            let total_notes = song.total_notes.get(diff_idx).copied().unwrap_or(0);
            let unlocked = get_unlock_state_for_difficulty(
                &self.game_data.unlock_state,
                &self.game_data.song_db,
                song_id,
                difficulty,
            );

            if let Err(e) = api
                .add_chart(&song_id_str, diff_idx as u8, level, total_notes, unlocked)
                .await
            {
                error!(
                    "Failed to add chart {:05}[{}]: {}",
                    song_id,
                    difficulty.short_name(),
                    e
                );
            }

            // Post initial score from score map
            if let Some(score_data) = self.game_data.score_map.get(song_id) {
                let ex_score = score_data.score[diff_idx];
                let miss_count = score_data.miss_count[diff_idx].unwrap_or(0);
                let lamp = score_data.lamp[diff_idx];
                let grade = if total_notes > 0 {
                    PlayData::calculate_grade(ex_score, total_notes)
                } else {
                    Grade::NoPlay
                };

                if let Err(e) = api
                    .post_score(
                        &song_id_str,
                        diff_idx as u8,
                        ex_score,
                        miss_count,
                        grade.short_name(),
                        lamp.short_name(),
                    )
                    .await
                {
                    error!(
                        "Failed to post score {:05}[{}]: {}",
                        song_id,
                        difficulty.short_name(),
                        e
                    );
                }
            }

            // Small delay to avoid overwhelming the server
            thread::sleep(Duration::from_millis(timing::SERVER_SYNC_REQUEST_DELAY_MS));
        }

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

    /// Update support files from update server
    ///
    /// Updates offsets.txt (if version matches), encodingfixes.txt, and customtypes.txt
    pub async fn update_support_files<P: AsRef<Path>>(
        &self,
        game_version: &str,
        base_dir: P,
    ) -> Result<UpdateResult> {
        if !self.config.update.update_files {
            info!("Support file updates disabled in config");
            return Ok(UpdateResult::default());
        }

        let update_server = &self.config.update.update_server;
        if update_server.is_empty() {
            warn!("No update server configured");
            return Ok(UpdateResult::default());
        }

        // Create API client for update server
        let api = RefluxApi::new(update_server.clone(), String::new())?;

        let base = base_dir.as_ref();
        let mut result = UpdateResult::default();

        // Try to update offsets for the game version
        let offsets_path = base.join("offsets.txt");
        match api.update_offsets(game_version, &offsets_path).await {
            Ok(true) => {
                info!("Updated offsets.txt to version {}", game_version);
                result.offsets_updated = true;
            }
            Ok(false) => {
                info!("No matching offsets available for version {}", game_version);
            }
            Err(e) => {
                warn!("Failed to check offsets update: {}", e);
            }
        }

        // Update encoding fixes
        let fixes_path = base.join("encodingfixes.txt");
        match api.update_support_file("encodingfixes", &fixes_path).await {
            Ok(true) => {
                info!("Updated encodingfixes.txt");
                result.encoding_fixes_updated = true;
            }
            Ok(false) => {}
            Err(e) => {
                warn!("Failed to update encodingfixes.txt: {}", e);
            }
        }

        // Update custom types
        let types_path = base.join("customtypes.txt");
        match api.update_support_file("customtypes", &types_path).await {
            Ok(true) => {
                info!("Updated customtypes.txt");
                result.custom_types_updated = true;
            }
            Ok(false) => {}
            Err(e) => {
                warn!("Failed to update customtypes.txt: {}", e);
            }
        }

        Ok(result)
    }
}
