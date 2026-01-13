use std::collections::HashMap;
use std::path::Path;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::error::Result;
use crate::game::{
    check_version_match, find_game_version, get_unlock_state_for_difficulty, get_unlock_states,
    ChartInfo, Difficulty, GameState, GameStateDetector, Grade, Judge, Lamp, PlayData, PlayType,
    Settings, SongInfo, UnlockData, UnlockType,
};
use crate::memory::{MemoryReader, ProcessHandle};
use crate::network::{AddSongParams, RefluxApi};
use crate::offset::OffsetsCollection;
use crate::storage::{format_post_form, SessionManager, Tracker, TrackerInfo, UnlockDb};
use crate::stream::StreamOutput;

/// Main Reflux application
pub struct Reflux {
    config: Config,
    offsets: OffsetsCollection,
    song_db: HashMap<String, SongInfo>,
    tracker: Tracker,
    state_detector: GameStateDetector,
    session_manager: SessionManager,
    stream_output: StreamOutput,
    api: Option<RefluxApi>,
    /// Persistent unlock state storage (from file)
    unlock_db: UnlockDb,
    /// Current unlock state from memory
    unlock_state: HashMap<String, UnlockData>,
}

impl Reflux {
    pub fn new(config: Config, offsets: OffsetsCollection) -> Self {
        let stream_output = StreamOutput::new(
            config.livestream.show_play_state
                || config.livestream.enable_marquee
                || config.livestream.enable_full_song_info
                || config.record.save_latest_json
                || config.record.save_latest_txt,
            ".".to_string(),
        );

        let api = if config.record.save_remote {
            Some(RefluxApi::new(
                config.remote_record.server_address.clone(),
                config.remote_record.api_key.clone(),
            ))
        } else {
            None
        };

        Self {
            config,
            offsets,
            song_db: HashMap::new(),
            tracker: Tracker::new(),
            state_detector: GameStateDetector::new(),
            session_manager: SessionManager::new("sessions"),
            stream_output,
            api,
            unlock_db: UnlockDb::new(),
            unlock_state: HashMap::new(),
        }
    }

    /// Load tracker data from file
    pub fn load_tracker<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        match Tracker::load(&path) {
            Ok(tracker) => {
                self.tracker = tracker;
                info!("Loaded tracker from {:?}", path.as_ref());
            }
            Err(e) => {
                warn!("Failed to load tracker: {}, starting fresh", e);
            }
        }
        Ok(())
    }

    /// Save tracker data to file
    pub fn save_tracker<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.tracker.save(path)?;
        Ok(())
    }

    /// Run the main tracking loop
    pub fn run(&mut self, process: &ProcessHandle) -> Result<()> {
        let reader = MemoryReader::new(process);
        let mut last_state = GameState::Unknown;

        info!("Starting tracker loop...");

        if self.config.record.save_local || self.config.record.save_json {
            self.session_manager = SessionManager::new("sessions");

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

        loop {
            // Check if process is still alive
            if reader.read_bytes(process.base_address, 4).is_err() {
                info!("Process terminated");
                break;
            }

            // Detect game state
            let current_state = self.detect_game_state(&reader)?;

            if current_state != last_state {
                info!("State changed: {:?} -> {:?}", last_state, current_state);
                self.handle_state_change(&reader, last_state, current_state)?;
                last_state = current_state;
            }

            thread::sleep(Duration::from_millis(100));
        }

        // Cleanup
        if self.config.livestream.show_play_state {
            let _ = self.stream_output.write_play_state(GameState::Unknown);
        }
        if self.config.livestream.enable_marquee {
            let _ = self.stream_output.write_marquee("NO SIGNAL");
        }

        Ok(())
    }

    fn detect_game_state(&mut self, reader: &MemoryReader) -> Result<GameState> {
        let word: u64 = 4;

        // Read markers for state detection
        let judge_marker_54 = reader
            .read_i32(self.offsets.judge_data + word * 54)
            .unwrap_or(0);
        let judge_marker_55 = reader
            .read_i32(self.offsets.judge_data + word * 55)
            .unwrap_or(0);
        let song_select_marker = reader
            .read_i32(self.offsets.play_settings - word * 6)
            .unwrap_or(0);

        Ok(self
            .state_detector
            .detect(judge_marker_54, judge_marker_55, song_select_marker))
    }

    fn handle_state_change(
        &mut self,
        reader: &MemoryReader,
        _old_state: GameState,
        new_state: GameState,
    ) -> Result<()> {
        match new_state {
            GameState::ResultScreen => {
                // Wait a bit to avoid race condition
                thread::sleep(Duration::from_secs(1));

                // Fetch play data
                match self.fetch_play_data(reader) {
                    Ok(play_data) => {
                        info!(
                            "Play data: {} {} - {} {}",
                            play_data.chart.title,
                            play_data.chart.difficulty.short_name(),
                            play_data.grade.short_name(),
                            play_data.lamp.short_name()
                        );

                        // Update tracker
                        self.update_tracker(&play_data);

                        // Save to session file (TSV and JSON)
                        if self.config.record.save_local {
                            // Append TSV row
                            if let Err(e) = self.session_manager.append_tsv_row(
                                &play_data,
                                &self.config.local_record,
                            ) {
                                error!("Failed to append TSV row: {}", e);
                            }
                        }

                        if self.config.record.save_json {
                            // Append JSON entry
                            if let Err(e) = self.session_manager.append_json_entry(&play_data) {
                                error!("Failed to append JSON entry: {}", e);
                            }
                        }

                        // Send to remote server
                        if self.config.record.save_remote
                            && let Some(api) = self.api.clone()
                        {
                            let form =
                                format_post_form(&play_data, &self.config.remote_record.api_key);
                            // Non-blocking send (fire and forget for now)
                            std::thread::spawn(move || {
                                let rt = tokio::runtime::Runtime::new().unwrap();
                                if let Err(e) = rt.block_on(api.report_play(form)) {
                                    tracing::error!("Failed to report play to remote: {}", e);
                                }
                            });
                        }

                        // Write latest files for OBS/streaming
                        let write_latest_json = self.config.record.save_latest_json;
                        let write_latest_txt = self.config.record.save_latest_txt;
                        if (write_latest_json || write_latest_txt)
                            && let Err(e) = self.stream_output.write_latest_files(
                                &play_data,
                                &self.config.remote_record.api_key,
                                write_latest_json,
                                write_latest_txt,
                            )
                        {
                            error!("Failed to write latest files: {}", e);
                        }

                        // Update streaming files
                        if self.config.livestream.show_play_state {
                            let _ = self.stream_output.write_play_state(GameState::ResultScreen);
                        }
                        if self.config.livestream.enable_marquee {
                            let status = if play_data.lamp == Lamp::Failed {
                                "FAIL!"
                            } else {
                                "CLEAR!"
                            };
                            let _ = self.stream_output.write_marquee(&format!(
                                "{} {}",
                                play_data.chart.title_english, status
                            ));
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch play data: {}", e);
                    }
                }
            }
            GameState::SongSelect => {
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
            }
            GameState::Playing => {
                // Fetch current chart info
                if let Ok((song_id, difficulty)) = self.fetch_current_chart(reader)
                    && let Some(song) = self.song_db.get(&song_id)
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
            GameState::Unknown => {}
        }

        Ok(())
    }

    /// Poll for unlock state changes and report to server
    fn poll_unlock_changes(&mut self, reader: &MemoryReader) {
        if !self.config.record.save_remote || self.api.is_none() {
            return;
        }

        if self.song_db.is_empty() {
            return;
        }

        // Read current unlock state
        let current_state =
            match get_unlock_states(reader, self.offsets.unlock_data, &self.song_db) {
            Ok(state) => state,
            Err(e) => {
                error!("Failed to read unlock state: {}", e);
                return;
            }
        };

        // Detect changes
        let changes = crate::game::detect_unlock_changes(&self.unlock_state, &current_state);

        if !changes.is_empty() {
            info!("Detected {} unlock state changes", changes.len());

            // Clone api before the loop to avoid borrow issues
            let api_opt = self.api.clone();

            // Report changes to server
            for (song_id, unlock_data) in &changes {
                if let Some(ref api) = api_opt {
                    let api_clone = api.clone();
                    let song_id_clone = song_id.clone();
                    let unlocks = unlock_data.unlocks;
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        if let Err(e) =
                            rt.block_on(api_clone.report_unlock(&song_id_clone, unlocks))
                        {
                            tracing::error!("Failed to report unlock for {}: {}", song_id_clone, e);
                        }
                    });
                }

                // Update local state
                self.unlock_db.update_from_data(song_id, unlock_data);
            }
        }

        // Update current unlock state
        self.unlock_state = current_state;
    }

    fn fetch_current_chart(&self, reader: &MemoryReader) -> Result<(String, Difficulty)> {
        let song_id = reader.read_i32(self.offsets.current_song)?;
        let diff = reader.read_i32(self.offsets.current_song + 4)?;

        let song_id_str = format!("{:05}", song_id);
        let difficulty =
            Difficulty::from_u8(diff as u8).unwrap_or(Difficulty::SpN);

        Ok((song_id_str, difficulty))
    }

    fn fetch_play_data(&self, reader: &MemoryReader) -> Result<PlayData> {
        let word: u64 = 4;

        // Read basic play data
        let song_id = reader.read_i32(self.offsets.play_data)?;
        let difficulty_val = reader.read_i32(self.offsets.play_data + word)?;
        let lamp_val = reader.read_i32(self.offsets.play_data + word * 6)?;

        let song_id_str = format!("{:05}", song_id);
        let difficulty =
            Difficulty::from_u8(difficulty_val as u8).unwrap_or(Difficulty::SpN);
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

        // Get or create chart info
        let chart = if let Some(song) = self.song_db.get(&song_id_str) {
            ChartInfo::from_song_info(song, difficulty, true)
        } else {
            // Create minimal chart info
            ChartInfo {
                song_id: song_id_str.clone(),
                title: format!("Song {}", song_id_str),
                title_english: format!("Song {}", song_id_str),
                artist: String::new(),
                genre: String::new(),
                bpm: String::new(),
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

        // Read gauge
        let gauge_p1 = reader
            .read_i32(self.offsets.judge_data + word * 81)
            .unwrap_or(0);
        let gauge_p2 = reader
            .read_i32(self.offsets.judge_data + word * 82)
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
            data_available: true,
        })
    }

    fn fetch_judge_data(&self, reader: &MemoryReader) -> Result<Judge> {
        let word: u64 = 4;
        let base = self.offsets.judge_data;

        let p1_pgreat = reader.read_u32(base)? ;
        let p1_great = reader.read_u32(base + word)?;
        let p1_good = reader.read_u32(base + word * 2)?;
        let p1_bad = reader.read_u32(base + word * 3)?;
        let p1_poor = reader.read_u32(base + word * 4)?;

        let p2_pgreat = reader.read_u32(base + word * 5)?;
        let p2_great = reader.read_u32(base + word * 6)?;
        let p2_good = reader.read_u32(base + word * 7)?;
        let p2_bad = reader.read_u32(base + word * 8)?;
        let p2_poor = reader.read_u32(base + word * 9)?;

        let p1_cb = reader.read_u32(base + word * 10)?;
        let p2_cb = reader.read_u32(base + word * 11)?;

        let p1_fast = reader.read_u32(base + word * 12)?;
        let p2_fast = reader.read_u32(base + word * 13)?;

        let p1_slow = reader.read_u32(base + word * 14)?;
        let p2_slow = reader.read_u32(base + word * 15)?;

        let p1_measure_end = reader.read_u32(base + word * 16)?;
        let p2_measure_end = reader.read_u32(base + word * 17)?;

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
            play_type,
            style_val,
            style2_val,
            gauge_val,
            assist_val,
            range_val,
            flip_val,
            battle_val,
            h_ran_val,
        ))
    }

    fn update_tracker(&mut self, play_data: &PlayData) {
        use crate::storage::ChartKey;

        // Parse song_id as u32
        let song_id: u32 = play_data.chart.song_id.parse().unwrap_or(0);

        let key = ChartKey {
            song_id,
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
            dj_points: 0.0, // TODO: Calculate DJ Points
        };

        self.tracker.update(key, new_info);
    }

    /// Set song database
    pub fn set_song_db(&mut self, song_db: HashMap<String, SongInfo>) {
        self.song_db = song_db;
    }

    /// Load unlock database from file
    pub fn load_unlock_db<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        match UnlockDb::load(&path) {
            Ok(db) => {
                info!("Loaded unlock db from {:?} ({} entries)", path.as_ref(), db.len());
                self.unlock_db = db;
            }
            Err(e) => {
                warn!("Failed to load unlock db: {}, starting fresh", e);
            }
        }
        Ok(())
    }

    /// Save unlock database to file
    pub fn save_unlock_db<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.unlock_db.save(path)?;
        Ok(())
    }

    /// Load current unlock state from memory
    pub fn load_unlock_state(&mut self, reader: &MemoryReader) -> Result<()> {
        if self.song_db.is_empty() {
            warn!("Song database is empty, cannot load unlock state");
            return Ok(());
        }

        self.unlock_state = get_unlock_states(reader, self.offsets.unlock_data, &self.song_db)?;
        info!("Loaded unlock state from memory ({} entries)", self.unlock_state.len());
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
            .song_db
            .keys()
            .filter(|id| !self.unlock_db.contains(id))
            .cloned()
            .collect();

        if !new_songs.is_empty() {
            info!("Found {} songs to upload to remote", new_songs.len());
        }

        for (i, song_id) in self.song_db.keys().enumerate() {
            let Some(song) = self.song_db.get(song_id) else {
                continue;
            };

            // Report progress for new songs
            if !new_songs.is_empty() && (i % 100 == 0 || i == self.song_db.len() - 1) {
                let percent = (i as f64 / self.song_db.len() as f64) * 100.0;
                info!("Sync progress: {:.1}%", percent);
            }

            // Upload new songs
            if !self.unlock_db.contains(song_id) {
                self.upload_song_info(api, song_id, song).await?;
            }

            // Check for unlock type/state changes
            if let Some(unlock_data) = self.unlock_state.get(song_id) {
                let current_type = match unlock_data.unlock_type {
                    UnlockType::Base => 1,
                    UnlockType::Bits => 2,
                    UnlockType::Sub => 3,
                };

                // Check unlock type change
                if self.unlock_db.has_unlock_type_changed(song_id, current_type) {
                    info!("Unlock type changed for {}: updating remote", song_id);
                    if let Err(e) = api
                        .update_chart_unlock_type(song_id, current_type as u8)
                        .await
                    {
                        error!("Failed to update unlock type for {}: {}", song_id, e);
                    }
                }

                // Check unlock state change
                if self.unlock_db.has_unlocks_changed(song_id, unlock_data.unlocks) {
                    info!("Unlock state changed for {}: reporting to remote", song_id);
                    if let Err(e) = api.report_unlock(song_id, unlock_data.unlocks).await {
                        error!("Failed to report unlock for {}: {}", song_id, e);
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
    async fn upload_song_info(
        &self,
        api: &RefluxApi,
        song_id: &str,
        song: &SongInfo,
    ) -> Result<()> {
        let unlock_type = self
            .unlock_state
            .get(song_id)
            .map(|u| match u.unlock_type {
                UnlockType::Base => 1,
                UnlockType::Bits => 2,
                UnlockType::Sub => 3,
            })
            .unwrap_or(1);

        // Add song
        let params = AddSongParams {
            song_id,
            title: &song.title,
            title_english: &song.title_english,
            artist: &song.artist,
            genre: &song.genre,
            bpm: &song.bpm,
            unlock_type,
        };

        if let Err(e) = api.add_song(params).await {
            error!("Failed to add song {}: {}", song_id, e);
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
                &self.unlock_state,
                &self.song_db,
                song_id,
                difficulty,
            );

            if let Err(e) = api
                .add_chart(song_id, diff_idx as u8, level, total_notes, unlocked)
                .await
            {
                error!(
                    "Failed to add chart {}[{}]: {}",
                    song_id,
                    difficulty.short_name(),
                    e
                );
            }

            // Small delay to avoid overwhelming the server
            thread::sleep(Duration::from_millis(20));
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
        let api = RefluxApi::new(update_server.clone(), String::new());

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

    /// Get the offsets version
    pub fn offsets_version(&self) -> &str {
        &self.offsets.version
    }

    /// Get a reference to the config
    pub fn config(&self) -> &Config {
        &self.config
    }
}

/// Result of support file updates
#[derive(Debug, Default)]
pub struct UpdateResult {
    pub offsets_updated: bool,
    pub encoding_fixes_updated: bool,
    pub custom_types_updated: bool,
}
