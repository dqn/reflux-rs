//! Main application logic for the Reflux score tracker.
//!
//! This module contains the core `Reflux` struct which orchestrates:
//! - Game state detection and tracking
//! - Score data collection from memory
//! - Session management and data export
//! - Integration with game memory via offsets
//!
//! ## Example
//!
//! ```ignore
//! let offsets = OffsetsCollection::default();
//! let mut reflux = Reflux::new(offsets);
//!
//! // Set up song database and score map
//! reflux.set_song_db(song_db);
//! reflux.set_score_map(score_map);
//!
//! // Run the tracking loop
//! reflux.run(&process, &running)?;
//! ```

mod game_loop;

use std::collections::HashMap;
use std::path::Path;

use tracing::{debug, info};

use crate::chart::{Difficulty, SongInfo, UnlockData};
use crate::error::Result;
use crate::offset::OffsetsCollection;
use crate::play::GameStateDetector;
use crate::score::ScoreMap;
use crate::export::StreamOutput;
use crate::session::SessionManager;

/// Game data loaded from memory and files
pub struct GameData {
    /// Song database loaded from game memory
    pub song_db: HashMap<u32, SongInfo>,
    /// Score map from game memory
    pub score_map: ScoreMap,
    /// Current unlock state from memory
    pub unlock_state: HashMap<u32, UnlockData>,
    /// Custom unlock types from customtypes.txt
    pub custom_types: HashMap<u32, String>,
}

impl GameData {
    fn new() -> Self {
        Self {
            song_db: HashMap::new(),
            score_map: ScoreMap::new(),
            unlock_state: HashMap::new(),
            custom_types: HashMap::new(),
        }
    }
}

/// Main Reflux application
pub struct Reflux {
    pub(crate) offsets: OffsetsCollection,
    /// Game data from memory
    pub(crate) game_data: GameData,
    pub(crate) state_detector: GameStateDetector,
    pub(crate) session_manager: SessionManager,
    /// Stream output for OBS integration.
    /// Reserved for future use - OBS file output during tracking.
    #[allow(dead_code)]
    pub(crate) stream_output: StreamOutput,
    /// Currently playing chart (set during Playing state)
    /// Used for cross-validation when fetching play data on ResultScreen
    pub(crate) current_playing: Option<(u32, Difficulty)>,
}

impl Reflux {
    pub fn new(offsets: OffsetsCollection) -> Self {
        let stream_output = StreamOutput::new(false, ".".to_string());

        // Log offset validation status
        if offsets.has_state_detection_offsets() {
            debug!(
                "State detection offsets: judge_data=0x{:X}, play_settings=0x{:X}",
                offsets.judge_data, offsets.play_settings
            );
        } else {
            info!(
                "State detection offsets not fully initialized: judge_data=0x{:X}, play_settings=0x{:X}",
                offsets.judge_data, offsets.play_settings
            );
        }

        Self {
            offsets,
            game_data: GameData::new(),
            state_detector: GameStateDetector::new(),
            session_manager: SessionManager::new("sessions"),
            stream_output,
            current_playing: None,
        }
    }

    /// Set score map
    pub fn set_score_map(&mut self, score_map: ScoreMap) {
        self.game_data.score_map = score_map;
    }

    /// Set custom types
    pub fn set_custom_types(&mut self, custom_types: HashMap<u32, String>) {
        self.game_data.custom_types = custom_types;
    }

    /// Set song database
    pub fn set_song_db(&mut self, song_db: HashMap<u32, SongInfo>) {
        self.game_data.song_db = song_db;
    }

    /// Get a reference to the offsets
    pub fn offsets(&self) -> &OffsetsCollection {
        &self.offsets
    }

    /// Get the offsets version
    pub fn offsets_version(&self) -> &str {
        &self.offsets.version
    }

    /// Update offsets while preserving tracker and game data
    ///
    /// This method updates the offsets without creating a new Reflux instance,
    /// preserving the loaded tracker data and game state.
    pub fn update_offsets(&mut self, offsets: OffsetsCollection) {
        if offsets.has_state_detection_offsets() {
            debug!(
                "Updated state detection offsets: judge_data=0x{:X}, play_settings=0x{:X}",
                offsets.judge_data, offsets.play_settings
            );
        }
        self.offsets = offsets;
    }

    /// Export tracker data to TSV file
    pub fn export_tracker_tsv<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        crate::export::export_tracker_tsv(
            path,
            &self.game_data.song_db,
            &self.game_data.unlock_state,
            &self.game_data.score_map,
            &self.game_data.custom_types,
        )
    }
}
