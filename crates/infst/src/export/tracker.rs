//! Tracker data export (TSV and JSON formats)

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::chart::{Difficulty, SongInfo, UnlockData, get_unlock_state_for_difficulty};
use crate::error::Result;
use crate::play::{PlayData, UnlockType, calculate_dj_points};
use crate::score::{Grade, Lamp, ScoreMap};

/// Chart data for JSON export
#[derive(Debug, Serialize)]
pub struct ChartDataJson {
    pub difficulty: String,
    pub level: u8,
    pub lamp: String,
    pub grade: String,
    pub ex_score: u32,
    pub miss_count: Option<u32>,
    pub total_notes: u32,
    pub dj_points: f64,
}

/// Song data for JSON export
#[derive(Debug, Serialize)]
pub struct SongDataJson {
    pub song_id: u32,
    pub title: String,
    pub artist: String,
    pub charts: Vec<ChartDataJson>,
}

/// Export data for JSON export
#[derive(Debug, Serialize)]
pub struct ExportDataJson {
    pub songs: Vec<SongDataJson>,
}

/// Generate detailed tracker TSV header
pub fn format_tracker_tsv_header() -> String {
    let mut columns = vec![
        "Song ID".to_string(),
        "Title".to_string(),
        "Type".to_string(),
        "Label".to_string(),
        "Cost Normal".to_string(),
        "Cost Hyper".to_string(),
        "Cost Another".to_string(),
        "SP DJ Points".to_string(),
        "DP DJ Points".to_string(),
    ];

    // Add columns for each difficulty (skipping DPB which doesn't exist)
    let difficulties = [
        "SPB", "SPN", "SPH", "SPA", "SPL", "DPN", "DPH", "DPA", "DPL",
    ];
    for diff in difficulties {
        columns.push(format!("{} Unlocked", diff));
        columns.push(format!("{} Rating", diff));
        columns.push(format!("{} Lamp", diff));
        columns.push(format!("{} Letter", diff));
        columns.push(format!("{} EX Score", diff));
        columns.push(format!("{} Miss Count", diff));
        columns.push(format!("{} Note Count", diff));
        columns.push(format!("{} DJ Points", diff));
    }

    columns.join("\t")
}

/// Export detailed tracker data to TSV
pub fn export_tracker_tsv<P: AsRef<Path>>(
    path: P,
    song_db: &HashMap<u32, SongInfo>,
    unlock_db: &HashMap<u32, UnlockData>,
    score_map: &ScoreMap,
) -> Result<()> {
    let mut lines = vec![format_tracker_tsv_header()];

    // Get all song IDs from song database (sorted)
    let mut song_ids: Vec<&u32> = song_db.keys().collect();
    song_ids.sort();

    for &song_id in song_ids {
        if let Some(entry) = generate_tracker_entry(song_id, song_db, unlock_db, score_map) {
            lines.push(entry);
        }
    }

    fs::write(path, lines.join("\n"))?;
    Ok(())
}

fn generate_tracker_entry(
    song_id: u32,
    song_db: &HashMap<u32, SongInfo>,
    unlock_db: &HashMap<u32, UnlockData>,
    score_map: &ScoreMap,
) -> Option<String> {
    let song = song_db.get(&song_id)?;
    let unlock = unlock_db.get(&song_id)?;
    let scores = score_map.get(song_id);

    let mut columns = Vec::new();

    // Song ID
    columns.push(song_id.to_string());

    // Title
    columns.push(song.title.to_string());

    // Type and Label (Label is same as Type)
    let type_name = match unlock.unlock_type {
        UnlockType::Base => "Base",
        UnlockType::Bits => "Bits",
        UnlockType::Sub => "Sub",
    };
    columns.push(type_name.to_string());
    columns.push(type_name.to_string()); // Label = Type

    // Bit costs (for N, H, A)
    for i in [1, 2, 3] {
        // SPN, SPH, SPA indices
        let cost = if unlock.unlock_type == UnlockType::Bits {
            let sp_level = song.levels[i] as i32;
            let dp_level = song.levels[i + 5] as i32; // DPN, DPH, DPA
            500 * (sp_level + dp_level)
        } else {
            0
        };
        columns.push(cost.to_string());
    }

    // SP and DP DJ Points (max of each)
    let mut sp_djp = 0.0f64;
    let mut dp_djp = 0.0f64;

    // Difficulty columns
    let difficulties = [
        Difficulty::SpB,
        Difficulty::SpN,
        Difficulty::SpH,
        Difficulty::SpA,
        Difficulty::SpL,
        Difficulty::DpN,
        Difficulty::DpH,
        Difficulty::DpA,
        Difficulty::DpL,
    ];

    let mut chart_data = Vec::new();
    for diff in &difficulties {
        let diff_index = *diff as usize;
        let unlocked = get_unlock_state_for_difficulty(unlock_db, song_db, song_id, *diff);
        let level = song.levels[diff_index];
        let total_notes = song.total_notes[diff_index];

        let (lamp, grade, ex_score, miss_count, djp) = if let Some(s) = scores {
            let lamp = s.lamp[diff_index];
            let ex_score = s.score[diff_index];
            let grade = if total_notes > 0 {
                PlayData::calculate_grade(ex_score, total_notes)
            } else {
                Grade::NoPlay
            };
            let djp = if total_notes > 0 {
                calculate_dj_points(ex_score, grade, lamp)
            } else {
                0.0
            };
            let miss_count = s.miss_count[diff_index];
            (lamp, grade, ex_score, miss_count, djp)
        } else {
            (Lamp::NoPlay, Grade::NoPlay, 0, None, 0.0)
        };

        // Track max DJ points for SP/DP
        if diff.is_sp() {
            sp_djp = sp_djp.max(djp);
        } else {
            dp_djp = dp_djp.max(djp);
        }

        chart_data.push((
            unlocked,
            level,
            lamp,
            grade,
            ex_score,
            miss_count,
            total_notes,
            djp,
        ));
    }

    // Add SP/DP DJ Points
    columns.push(if sp_djp > 0.0 {
        format!("{}", sp_djp)
    } else {
        String::new()
    });
    columns.push(if dp_djp > 0.0 {
        format!("{}", dp_djp)
    } else {
        String::new()
    });

    // Add chart data columns
    for (unlocked, level, lamp, grade, ex_score, miss_count, total_notes, djp) in chart_data {
        columns.push(if unlocked { "TRUE" } else { "FALSE" }.to_string());
        columns.push(level.to_string());
        columns.push(lamp.short_name().to_string());
        columns.push(grade.short_name().to_string());
        columns.push(ex_score.to_string());
        columns.push(
            miss_count
                .map(|m| m.to_string())
                .unwrap_or_else(|| "-".to_string()),
        );
        columns.push(total_notes.to_string());
        columns.push(if djp > 0.0 {
            format!("{}", djp)
        } else {
            String::new()
        });
    }

    Some(columns.join("\t"))
}

/// Export song database to TSV for debugging
///
/// Format: id, title, title2 (English), artist, genre
/// Useful for checking encoding issues
pub fn export_song_list<P: AsRef<Path>>(path: P, song_db: &HashMap<u32, SongInfo>) -> Result<()> {
    let mut lines = vec!["id\ttitle\ttitle2\tartist\tgenre".to_string()];

    // Sort by song ID
    let mut song_ids: Vec<&u32> = song_db.keys().collect();
    song_ids.sort();

    for &song_id in song_ids {
        if let Some(song) = song_db.get(&song_id) {
            lines.push(format!(
                "{:05}\t{}\t{}\t{}\t{}",
                song_id, song.title, song.title_english, song.artist, song.genre
            ));
        }
    }

    fs::write(path, lines.join("\n"))?;
    Ok(())
}

/// Export detailed tracker data to JSON
pub fn export_tracker_json<P: AsRef<Path>>(
    path: P,
    song_db: &HashMap<u32, SongInfo>,
    unlock_db: &HashMap<u32, UnlockData>,
    score_map: &ScoreMap,
) -> Result<()> {
    let content = generate_tracker_json(song_db, unlock_db, score_map)?;
    fs::write(path, content)?;
    Ok(())
}

/// Generate tracker JSON string (for stdout output)
pub fn generate_tracker_json(
    song_db: &HashMap<u32, SongInfo>,
    unlock_db: &HashMap<u32, UnlockData>,
    score_map: &ScoreMap,
) -> Result<String> {
    let mut songs = Vec::new();

    // Get all song IDs from song database (sorted)
    let mut song_ids: Vec<&u32> = song_db.keys().collect();
    song_ids.sort();

    for &song_id in song_ids {
        if let Some(song_data) = generate_song_json(song_id, song_db, unlock_db, score_map) {
            songs.push(song_data);
        }
    }

    let export_data = ExportDataJson { songs };
    let json = serde_json::to_string_pretty(&export_data)?;
    Ok(json)
}

fn generate_song_json(
    song_id: u32,
    song_db: &HashMap<u32, SongInfo>,
    unlock_db: &HashMap<u32, UnlockData>,
    score_map: &ScoreMap,
) -> Option<SongDataJson> {
    let song = song_db.get(&song_id)?;
    let _unlock = unlock_db.get(&song_id)?;
    let scores = score_map.get(song_id);

    let difficulties = [
        Difficulty::SpB,
        Difficulty::SpN,
        Difficulty::SpH,
        Difficulty::SpA,
        Difficulty::SpL,
        Difficulty::DpN,
        Difficulty::DpH,
        Difficulty::DpA,
        Difficulty::DpL,
    ];

    let mut charts = Vec::new();
    for diff in &difficulties {
        let diff_index = *diff as usize;
        let level = song.levels[diff_index];
        let total_notes = song.total_notes[diff_index];

        // Skip charts with no notes (non-existent difficulty)
        if total_notes == 0 {
            continue;
        }

        let (lamp, grade, ex_score, miss_count, djp) = if let Some(s) = scores {
            let lamp = s.lamp[diff_index];
            let ex_score = s.score[diff_index];
            let grade = PlayData::calculate_grade(ex_score, total_notes);
            let djp = calculate_dj_points(ex_score, grade, lamp);
            let miss_count = s.miss_count[diff_index];
            (lamp, grade, ex_score, miss_count, djp)
        } else {
            (Lamp::NoPlay, Grade::NoPlay, 0, None, 0.0)
        };

        charts.push(ChartDataJson {
            difficulty: diff.short_name().to_string(),
            level,
            lamp: lamp.expand_name().to_string(),
            grade: grade.short_name().to_string(),
            ex_score,
            miss_count,
            total_notes,
            dj_points: djp,
        });
    }

    Some(SongDataJson {
        song_id,
        title: song.title.to_string(),
        artist: song.artist.to_string(),
        charts,
    })
}

/// Generate tracker TSV string (for stdout output)
pub fn generate_tracker_tsv(
    song_db: &HashMap<u32, SongInfo>,
    unlock_db: &HashMap<u32, UnlockData>,
    score_map: &ScoreMap,
) -> String {
    let mut lines = vec![format_tracker_tsv_header()];

    // Get all song IDs from song database (sorted)
    let mut song_ids: Vec<&u32> = song_db.keys().collect();
    song_ids.sort();

    for &song_id in song_ids {
        if let Some(entry) = generate_tracker_entry(song_id, song_db, unlock_db, score_map) {
            lines.push(entry);
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_test_song(id: u32, title: &str) -> SongInfo {
        SongInfo {
            id,
            title: Arc::from(title),
            title_english: Arc::from(""),
            artist: Arc::from("Test Artist"),
            genre: Arc::from("Test Genre"),
            bpm: Arc::from("150"),
            folder: 1,
            levels: [0, 5, 8, 10, 12, 0, 5, 8, 10, 12],
            total_notes: [0, 500, 800, 1000, 1200, 0, 500, 800, 1000, 1200],
            unlock_type: UnlockType::Base,
        }
    }

    #[test]
    fn test_format_tracker_tsv_header() {
        let header = format_tracker_tsv_header();
        assert!(header.contains("Song ID"));
        assert!(header.contains("Title"));
        assert!(header.contains("Type"));
        assert!(header.contains("Label"));
        assert!(header.contains("SP DJ Points"));
        assert!(header.contains("DP DJ Points"));
        assert!(header.contains("SPA Lamp"));
        assert!(header.contains("DPA Lamp"));
    }

    #[test]
    fn test_generate_tracker_json_empty() {
        let song_db: HashMap<u32, SongInfo> = HashMap::new();
        let unlock_db: HashMap<u32, UnlockData> = HashMap::new();
        let score_map = ScoreMap::new();

        let json = generate_tracker_json(&song_db, &unlock_db, &score_map).unwrap();

        // Check output contains expected structure
        assert!(json.contains("\"songs\""));
        assert!(json.contains("[]"));
    }

    #[test]
    fn test_generate_tracker_json_with_song() {
        let mut song_db: HashMap<u32, SongInfo> = HashMap::new();
        song_db.insert(1000, create_test_song(1000, "Test Song"));

        let mut unlock_db: HashMap<u32, UnlockData> = HashMap::new();
        unlock_db.insert(
            1000,
            UnlockData {
                song_id: 1000,
                unlock_type: UnlockType::Base,
                unlocks: 0x3FF, // All 10 difficulties unlocked
            },
        );

        let score_map = ScoreMap::new();

        let json = generate_tracker_json(&song_db, &unlock_db, &score_map).unwrap();

        // Verify JSON structure contains expected data
        assert!(json.contains("\"song_id\": 1000"));
        assert!(json.contains("\"title\": \"Test Song\""));
    }

    #[test]
    fn test_generate_tracker_tsv_header_only_when_empty() {
        let song_db: HashMap<u32, SongInfo> = HashMap::new();
        let unlock_db: HashMap<u32, UnlockData> = HashMap::new();
        let score_map = ScoreMap::new();

        let tsv = generate_tracker_tsv(&song_db, &unlock_db, &score_map);
        let lines: Vec<&str> = tsv.lines().collect();

        // Should only have header
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Title"));
    }
}
