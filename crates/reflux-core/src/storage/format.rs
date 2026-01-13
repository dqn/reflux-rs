use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::{json, Value as JsonValue};

use crate::config::LocalRecordConfig;
use crate::error::Result;
use crate::game::{
    calculate_dj_points, get_unlock_state_for_difficulty, Difficulty, Grade, Lamp, PlayData,
    SongInfo, UnlockData, UnlockType,
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

/// Generate dynamic TSV header based on config flags
pub fn format_dynamic_tsv_header(config: &LocalRecordConfig) -> String {
    let mut columns = vec!["title", "difficulty"];

    if config.song_info {
        columns.extend(["title2", "bpm", "artist", "genre"]);
    }
    if config.chart_details {
        columns.extend(["notecount", "level"]);
    }

    columns.extend(["playtype", "grade", "lamp", "misscount"]);

    if config.result_details {
        columns.extend(["gaugepercent", "exscore"]);
    }
    if config.judge {
        columns.extend([
            "pgreat",
            "great",
            "good",
            "bad",
            "poor",
            "combobreak",
            "fast",
            "slow",
        ]);
    }
    if config.settings {
        columns.extend(["style", "style2", "gauge", "assist", "range"]);
    }

    columns.push("date");

    columns.join("\t")
}

/// Generate dynamic TSV row based on config flags
pub fn format_dynamic_tsv_row(play_data: &PlayData, config: &LocalRecordConfig) -> String {
    let mut values: Vec<String> = vec![
        play_data.chart.title.clone(),
        play_data.chart.difficulty.short_name().to_string(),
    ];

    if config.song_info {
        values.push(play_data.chart.title_english.clone());
        values.push(play_data.chart.bpm.clone());
        values.push(play_data.chart.artist.clone());
        values.push(play_data.chart.genre.clone());
    }
    if config.chart_details {
        values.push(play_data.chart.total_notes.to_string());
        values.push(play_data.chart.level.to_string());
    }

    values.push(play_data.judge.play_type.short_name().to_string());
    values.push(play_data.grade.short_name().to_string());
    values.push(play_data.lamp.short_name().to_string());
    values.push(if play_data.miss_count_valid() {
        play_data.miss_count().to_string()
    } else {
        "-".to_string()
    });

    if config.result_details {
        values.push(play_data.gauge.to_string());
        values.push(play_data.ex_score.to_string());
    }
    if config.judge {
        values.push(play_data.judge.pgreat.to_string());
        values.push(play_data.judge.great.to_string());
        values.push(play_data.judge.good.to_string());
        values.push(play_data.judge.bad.to_string());
        values.push(play_data.judge.poor.to_string());
        values.push(play_data.judge.combo_break.to_string());
        values.push(play_data.judge.fast.to_string());
        values.push(play_data.judge.slow.to_string());
    }
    if config.settings {
        values.push(play_data.settings.style.as_str().to_string());
        values.push(
            play_data
                .settings
                .style2
                .map(|s| s.as_str())
                .unwrap_or("OFF")
                .to_string(),
        );
        values.push(play_data.settings.gauge.as_str().to_string());
        values.push(play_data.settings.assist.as_str().to_string());
        values.push(play_data.settings.range.as_str().to_string());
    }

    values.push(play_data.timestamp.to_rfc3339());

    values.join("\t")
}

/// Generate JSON entry for session file (Kamaitachi format)
pub fn format_json_entry(play_data: &PlayData) -> JsonValue {
    let playtype = if play_data.chart.difficulty.is_sp() {
        "SP"
    } else {
        "DP"
    };

    let mut entry = json!({
        "score": play_data.ex_score,
        "lamp": play_data.lamp.expand_name(),
        "matchType": "title",
        "identifier": play_data.chart.title,
        "playtype": playtype,
        "difficulty": play_data.chart.difficulty.expand_name(),
        "timeAchieved": play_data.timestamp.timestamp_millis(),
        "hitData": {
            "pgreat": play_data.judge.pgreat,
            "great": play_data.judge.great,
            "good": play_data.judge.good,
            "bad": play_data.judge.bad,
            "poor": play_data.judge.poor
        },
        "hitMeta": {
            "fast": play_data.judge.fast,
            "slow": play_data.judge.slow,
            "comboBreak": play_data.judge.combo_break,
            "gauge": play_data.gauge
        }
    });

    if play_data.miss_count_valid() {
        entry["hitMeta"]["bp"] = json!(play_data.miss_count());
    }

    entry
}

/// Generate post form data for remote server
pub fn format_post_form(play_data: &PlayData, api_key: &str) -> HashMap<String, String> {
    let mut form = HashMap::new();

    form.insert("apikey".to_string(), api_key.to_string());
    form.insert("songid".to_string(), play_data.chart.song_id.clone());
    form.insert("title".to_string(), play_data.chart.title.clone());
    form.insert("title2".to_string(), play_data.chart.title_english.clone());
    form.insert("bpm".to_string(), play_data.chart.bpm.clone());
    form.insert("artist".to_string(), play_data.chart.artist.clone());
    form.insert("genre".to_string(), play_data.chart.genre.clone());
    form.insert(
        "notecount".to_string(),
        play_data.chart.total_notes.to_string(),
    );
    form.insert(
        "diff".to_string(),
        play_data.chart.difficulty.short_name().to_string(),
    );
    form.insert("level".to_string(), play_data.chart.level.to_string());
    form.insert(
        "unlocked".to_string(),
        play_data.chart.unlocked.to_string(),
    );
    form.insert("grade".to_string(), play_data.grade.short_name().to_string());
    form.insert("gaugepercent".to_string(), play_data.gauge.to_string());
    form.insert("lamp".to_string(), play_data.lamp.short_name().to_string());
    form.insert("exscore".to_string(), play_data.ex_score.to_string());
    form.insert(
        "prematureend".to_string(),
        play_data.judge.premature_end.to_string(),
    );
    form.insert("pgreat".to_string(), play_data.judge.pgreat.to_string());
    form.insert("great".to_string(), play_data.judge.great.to_string());
    form.insert("good".to_string(), play_data.judge.good.to_string());
    form.insert("bad".to_string(), play_data.judge.bad.to_string());
    form.insert("poor".to_string(), play_data.judge.poor.to_string());
    form.insert("fast".to_string(), play_data.judge.fast.to_string());
    form.insert("slow".to_string(), play_data.judge.slow.to_string());
    form.insert(
        "combobreak".to_string(),
        play_data.judge.combo_break.to_string(),
    );
    form.insert(
        "playtype".to_string(),
        play_data.judge.play_type.short_name().to_string(),
    );
    form.insert(
        "style".to_string(),
        play_data.settings.style.as_str().to_string(),
    );
    form.insert(
        "style2".to_string(),
        play_data
            .settings
            .style2
            .map(|s| s.as_str())
            .unwrap_or("OFF")
            .to_string(),
    );
    form.insert(
        "gauge".to_string(),
        play_data.settings.gauge.as_str().to_string(),
    );
    form.insert(
        "assist".to_string(),
        play_data.settings.assist.as_str().to_string(),
    );
    form.insert(
        "range".to_string(),
        play_data.settings.range.as_str().to_string(),
    );

    form
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

/// Export song database to TSV for debugging
///
/// Format: id, title, title2 (English), artist, genre
/// Useful for checking encoding issues
pub fn export_song_list<P: AsRef<Path>>(
    path: P,
    song_db: &HashMap<String, SongInfo>,
) -> Result<()> {
    let mut lines = vec!["id\ttitle\ttitle2\tartist\tgenre".to_string()];

    // Sort by song ID
    let mut song_ids: Vec<&String> = song_db.keys().collect();
    song_ids.sort();

    for song_id in song_ids {
        if let Some(song) = song_db.get(song_id) {
            lines.push(format!(
                "{}\t{}\t{}\t{}\t{}",
                song_id, song.title, song.title_english, song.artist, song.genre
            ));
        }
    }

    fs::write(path, lines.join("\n"))?;
    Ok(())
}

/// Format play data for console display with aligned columns
///
/// Returns a multi-line string with header and values aligned
pub fn format_play_data_console(play_data: &PlayData) -> String {
    let mut lines = Vec::new();
    lines.push("\nLATEST CLEAR:".to_string());

    // Build key-value pairs
    let pairs = [
        ("Title", play_data.chart.title.clone()),
        ("Difficulty", play_data.chart.difficulty.short_name().to_string()),
        ("Level", play_data.chart.level.to_string()),
        ("EX Score", play_data.ex_score.to_string()),
        ("Grade", play_data.grade.short_name().to_string()),
        ("Lamp", play_data.lamp.short_name().to_string()),
        ("PGreat", play_data.judge.pgreat.to_string()),
        ("Great", play_data.judge.great.to_string()),
        ("Good", play_data.judge.good.to_string()),
        ("Bad", play_data.judge.bad.to_string()),
        ("Poor", play_data.judge.poor.to_string()),
        ("Fast", play_data.judge.fast.to_string()),
        ("Slow", play_data.judge.slow.to_string()),
        ("ComboBreak", play_data.judge.combo_break.to_string()),
        ("Gauge", format!("{}%", play_data.gauge)),
        ("PlayType", play_data.judge.play_type.short_name().to_string()),
        ("Style", play_data.settings.style.as_str().to_string()),
        ("Gauge Type", play_data.settings.gauge.as_str().to_string()),
    ];

    for (key, value) in pairs {
        lines.push(format!("{:>15}: {:<50}", key, value));
    }

    lines.join("\n")
}

/// Simple play data summary for logging
pub fn format_play_summary(play_data: &PlayData) -> String {
    format!(
        "{} {} {} {} (EX:{}) {}",
        play_data.chart.title,
        play_data.chart.difficulty.short_name(),
        play_data.grade.short_name(),
        play_data.lamp.short_name(),
        play_data.ex_score,
        if play_data.data_available { "" } else { "[INVALID]" }
    )
}
