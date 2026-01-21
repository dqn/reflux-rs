use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OffsetsCollection {
    pub version: String,
    pub song_list: u64,
    pub data_map: u64,
    pub judge_data: u64,
    pub play_data: u64,
    pub play_settings: u64,
    pub unlock_data: u64,
    pub current_song: u64,
}

impl OffsetsCollection {
    /// Check if all required offsets are valid
    pub fn is_valid(&self) -> bool {
        !self.version.is_empty()
            && self.song_list != 0
            && self.data_map != 0
            && self.judge_data != 0
            && self.play_data != 0
            && self.play_settings != 0
            && self.unlock_data != 0
            && self.current_song != 0
    }

    /// Check if offsets required for state detection are valid
    pub fn has_state_detection_offsets(&self) -> bool {
        self.judge_data != 0 && self.play_settings != 0
    }
}
