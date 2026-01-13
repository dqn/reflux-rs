use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::Result;
use crate::game::{
    calculate_dj_points, get_unlock_state_for_difficulty, Difficulty, Grade, Lamp, SongInfo,
    UnlockData, UnlockType,
};
use crate::storage::{ScoreMap, Tracker};

pub fn format_tsv_header() -> String {
    [
        "Timestamp",
        "Title",
        "Difficulty",
        "Level",
        "EX Score",
        "Grade",
        "Lamp",
        "PGreat",
        "Great",
        "Good",
        "Bad",
        "Poor",
        "Fast",
        "Slow",
        "ComboBreak",
    ]
    .join("\t")
}

pub struct TsvRowData<'a> {
    pub timestamp: &'a str,
    pub title: &'a str,
    pub difficulty: &'a str,
    pub level: u8,
    pub ex_score: u32,
    pub grade: &'a str,
    pub lamp: &'a str,
    pub pgreat: u32,
    pub great: u32,
    pub good: u32,
    pub bad: u32,
    pub poor: u32,
    pub fast: u32,
    pub slow: u32,
    pub combo_break: u32,
}

pub fn format_tsv_row(data: &TsvRowData) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        data.timestamp,
        data.title,
        data.difficulty,
        data.level,
        data.ex_score,
        data.grade,
        data.lamp,
        data.pgreat,
        data.great,
        data.good,
        data.bad,
        data.poor,
        data.fast,
        data.slow,
        data.combo_break
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayDataJson {
    pub timestamp: String,
    pub song_id: String,
    pub title: String,
    pub difficulty: String,
    pub level: u8,
    pub ex_score: u32,
    pub grade: String,
    pub lamp: String,
    pub judge: JudgeJson,
}

#[derive(Debug, Clone, Serialize)]
pub struct JudgeJson {
    pub pgreat: u32,
    pub great: u32,
    pub good: u32,
    pub bad: u32,
    pub poor: u32,
    pub fast: u32,
    pub slow: u32,
    pub combo_break: u32,
}

/// Generate detailed tracker TSV header
pub fn format_tracker_tsv_header() -> String {
    let mut columns = vec![
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
    let difficulties = ["SPB", "SPN", "SPH", "SPA", "SPL", "DPN", "DPH", "DPA", "DPL"];
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
    _tracker: &Tracker,
    song_db: &HashMap<String, SongInfo>,
    unlock_db: &HashMap<String, UnlockData>,
    score_map: &ScoreMap,
    custom_types: &HashMap<String, String>,
) -> Result<()> {
    let mut lines = vec![format_tracker_tsv_header()];

    // Get all song IDs from song database (sorted)
    let mut song_ids: Vec<&String> = song_db.keys().collect();
    song_ids.sort();

    for song_id in song_ids {
        if let Some(entry) = generate_tracker_entry(
            song_id,
            song_db,
            unlock_db,
            score_map,
            custom_types,
        ) {
            lines.push(entry);
        }
    }

    fs::write(path, lines.join("\n"))?;
    Ok(())
}

fn generate_tracker_entry(
    song_id: &str,
    song_db: &HashMap<String, SongInfo>,
    unlock_db: &HashMap<String, UnlockData>,
    score_map: &ScoreMap,
    custom_types: &HashMap<String, String>,
) -> Option<String> {
    let song = song_db.get(song_id)?;
    let unlock = unlock_db.get(song_id)?;
    let scores = score_map.get(song_id);

    let mut columns = Vec::new();

    // Title
    columns.push(song.title.clone());

    // Type and Label
    let type_name = match unlock.unlock_type {
        UnlockType::Base => "Base",
        UnlockType::Bits => "Bits",
        UnlockType::Sub => "Sub",
    };
    columns.push(type_name.to_string());

    let label = custom_types
        .get(song_id)
        .cloned()
        .unwrap_or_else(|| type_name.to_string());
    columns.push(label);

    // Bit costs (for N, H, A)
    for i in [1, 2, 3] { // SPN, SPH, SPA indices
        let cost = if unlock.unlock_type == UnlockType::Bits && !custom_types.contains_key(song_id) {
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
        (0, Difficulty::SpB),
        (1, Difficulty::SpN),
        (2, Difficulty::SpH),
        (3, Difficulty::SpA),
        (4, Difficulty::SpL),
        (5, Difficulty::DpN),
        (6, Difficulty::DpH),
        (7, Difficulty::DpA),
        (8, Difficulty::DpL),
    ];

    let mut chart_data = Vec::new();
    for (idx, diff) in &difficulties {
        let unlocked = get_unlock_state_for_difficulty(unlock_db, song_db, song_id, *diff);
        let level = song.levels[*idx];
        let total_notes = song.total_notes[*idx];

        let (lamp, grade, ex_score, miss_count, djp) = if let Some(s) = scores {
            let lamp = s.lamp[*idx];
            let ex_score = s.score[*idx];
            let grade = if total_notes > 0 {
                crate::game::PlayData::calculate_grade(ex_score, total_notes)
            } else {
                Grade::NoPlay
            };
            let djp = if total_notes > 0 {
                calculate_dj_points(ex_score, grade, lamp)
            } else {
                0.0
            };
            let miss_count = s.miss_count[*idx];
            (lamp, grade, ex_score, miss_count, djp)
        } else {
            (Lamp::NoPlay, Grade::NoPlay, 0, None, 0.0)
        };

        // Track max DJ points for SP/DP
        if *idx < 5 {
            sp_djp = sp_djp.max(djp);
        } else {
            dp_djp = dp_djp.max(djp);
        }

        chart_data.push((unlocked, level, lamp, grade, ex_score, miss_count, total_notes, djp));
    }

    // Add SP/DP DJ Points
    columns.push(if sp_djp > 0.0 { format!("{:.8E}", sp_djp) } else { String::new() });
    columns.push(if dp_djp > 0.0 { format!("{:.8E}", dp_djp) } else { String::new() });

    // Add chart data columns
    for (unlocked, level, lamp, grade, ex_score, miss_count, total_notes, djp) in chart_data {
        columns.push(if unlocked { "TRUE" } else { "FALSE" }.to_string());
        columns.push(level.to_string());
        columns.push(lamp.short_name().to_string());
        columns.push(grade.short_name().to_string());
        columns.push(ex_score.to_string());
        columns.push(miss_count.map(|m| m.to_string()).unwrap_or_else(|| "-".to_string()));
        columns.push(total_notes.to_string());
        columns.push(if djp > 0.0 { format!("{:.8E}", djp) } else { String::new() });
    }

    Some(columns.join("\t"))
}
