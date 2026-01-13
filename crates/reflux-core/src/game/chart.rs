use serde::{Deserialize, Serialize};

use crate::game::{Difficulty, SongInfo};

/// Chart identifier (song + difficulty)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chart {
    pub song_id: u32,
    pub difficulty: Difficulty,
}

/// Full chart information including song metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartInfo {
    pub song_id: u32,
    pub title: String,
    pub title_english: String,
    pub artist: String,
    pub genre: String,
    pub bpm: String,
    pub difficulty: Difficulty,
    pub level: u8,
    pub total_notes: u32,
    pub unlocked: bool,
}

impl ChartInfo {
    pub fn from_song_info(song: &SongInfo, difficulty: Difficulty, unlocked: bool) -> Self {
        let diff_index = difficulty as usize;
        Self {
            song_id: song.id,
            title: song.title.clone(),
            title_english: song.title_english.clone(),
            artist: song.artist.clone(),
            genre: song.genre.clone(),
            bpm: song.bpm.clone(),
            difficulty,
            level: song.get_level(diff_index),
            total_notes: song.get_total_notes(diff_index),
            unlocked,
        }
    }

    /// Calculate max EX score (total_notes * 2)
    pub fn max_ex_score(&self) -> u32 {
        self.total_notes * 2
    }
}
