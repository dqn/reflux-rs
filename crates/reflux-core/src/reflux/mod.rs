mod game_loop;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::runtime::Handle;
use tracing::{info, warn};

use crate::config::Config;
use crate::error::{ApiErrorTracker, Result};
use crate::game::{GameStateDetector, SongInfo, UnlockData};
use crate::network::RefluxApi;
use crate::offset::OffsetsCollection;
use crate::storage::{ScoreMap, SessionManager, Tracker, UnlockDb};
use crate::stream::StreamOutput;

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
    pub(crate) config: Config,
    pub(crate) offsets: OffsetsCollection,
    /// Game data from memory
    pub(crate) game_data: GameData,
    pub(crate) tracker: Tracker,
    pub(crate) state_detector: GameStateDetector,
    pub(crate) session_manager: SessionManager,
    pub(crate) stream_output: StreamOutput,
    pub(crate) api: Option<RefluxApi>,
    /// Persistent unlock state storage (from file)
    pub(crate) unlock_db: UnlockDb,
    /// Tokio runtime handle for spawning async tasks
    pub(crate) runtime_handle: Option<Handle>,
    /// Tracker for API errors during session
    pub(crate) api_error_tracker: Arc<ApiErrorTracker>,
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
            match RefluxApi::new(
                config.remote_record.server_address.clone(),
                config.remote_record.api_key.clone(),
            ) {
                Ok(api) => Some(api),
                Err(e) => {
                    warn!("Failed to create API client: {}, remote saving disabled", e);
                    None
                }
            }
        } else {
            None
        };

        // Try to get the current tokio runtime handle if one exists
        let runtime_handle = Handle::try_current().ok();

        Self {
            config,
            offsets,
            game_data: GameData::new(),
            tracker: Tracker::new(),
            state_detector: GameStateDetector::new(),
            session_manager: SessionManager::new("sessions"),
            stream_output,
            api,
            unlock_db: UnlockDb::new(),
            runtime_handle,
            api_error_tracker: Arc::new(ApiErrorTracker::new()),
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

    /// Load tracker data from file
    pub fn load_tracker<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        match Tracker::load(&path) {
            Ok(tracker) => {
                self.tracker = tracker;
                info!("Loaded tracker from {:?}", path.as_ref());
            }
            Err(e) => {
                if e.is_not_found() {
                    info!("Tracker file not found, starting fresh");
                } else {
                    warn!("Failed to load tracker: {}, starting fresh", e);
                }
            }
        }
        Ok(())
    }

    /// Save tracker data to file
    pub fn save_tracker<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.tracker.save(path)?;
        Ok(())
    }

    /// Set song database
    pub fn set_song_db(&mut self, song_db: HashMap<u32, SongInfo>) {
        self.game_data.song_db = song_db;
    }

    /// Load unlock database from file
    pub fn load_unlock_db<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        match UnlockDb::load(&path) {
            Ok(db) => {
                info!(
                    "Loaded unlock db from {:?} ({} entries)",
                    path.as_ref(),
                    db.len()
                );
                self.unlock_db = db;
            }
            Err(e) => {
                if e.is_not_found() {
                    info!("Unlock db file not found, starting fresh");
                } else {
                    warn!("Failed to load unlock db: {}, starting fresh", e);
                }
            }
        }
        Ok(())
    }

    /// Save unlock database to file
    pub fn save_unlock_db<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.unlock_db.save(path)?;
        Ok(())
    }

    /// Get a reference to the offsets
    pub fn offsets(&self) -> &OffsetsCollection {
        &self.offsets
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
