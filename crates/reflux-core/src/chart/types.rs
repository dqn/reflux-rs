use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::chart::{Difficulty, SongInfo};

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
    pub title: Arc<str>,
    pub title_english: Arc<str>,
    pub artist: Arc<str>,
    pub genre: Arc<str>,
    pub bpm: Arc<str>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::UnlockType;

    fn make_test_song() -> SongInfo {
        let mut notes = [0u32; 10];
        notes[0] = 500; // SPB
        notes[1] = 600; // SPN
        notes[2] = 800; // SPH
        notes[3] = 1200; // SPA
        notes[4] = 1500; // SPL
        notes[5] = 550; // DPB
        notes[6] = 650; // DPN
        notes[7] = 850; // DPH
        notes[8] = 1250; // DPA
        notes[9] = 1550; // DPL

        let mut levels = [0u8; 10];
        levels[0] = 3; // SPB
        levels[1] = 5; // SPN
        levels[2] = 8; // SPH
        levels[3] = 11; // SPA
        levels[4] = 12; // SPL

        SongInfo {
            id: 1001,
            title: Arc::from("Test Song"),
            title_english: Arc::from("Test Song EN"),
            artist: Arc::from("Test Artist"),
            genre: Arc::from("Test Genre"),
            bpm: Arc::from("150"),
            folder: 1,
            levels,
            total_notes: notes,
            unlock_type: UnlockType::Base,
        }
    }

    #[test]
    fn test_chart_info_from_song_info() {
        let song = make_test_song();
        let chart = ChartInfo::from_song_info(&song, Difficulty::SpA, true);

        assert_eq!(chart.song_id, 1001);
        assert_eq!(&*chart.title, "Test Song");
        assert_eq!(chart.difficulty, Difficulty::SpA);
        assert_eq!(chart.level, 11);
        assert_eq!(chart.total_notes, 1200);
        assert!(chart.unlocked);
    }

    #[test]
    fn test_chart_info_max_ex_score() {
        let song = make_test_song();
        let chart = ChartInfo::from_song_info(&song, Difficulty::SpA, true);

        assert_eq!(chart.max_ex_score(), 2400); // 1200 * 2
    }

    #[test]
    fn test_chart_equality() {
        let chart1 = Chart {
            song_id: 1001,
            difficulty: Difficulty::SpA,
        };
        let chart2 = Chart {
            song_id: 1001,
            difficulty: Difficulty::SpA,
        };
        let chart3 = Chart {
            song_id: 1001,
            difficulty: Difficulty::SpH,
        };

        assert_eq!(chart1, chart2);
        assert_ne!(chart1, chart3);
    }
}
