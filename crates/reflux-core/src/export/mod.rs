//! Export formats for play data and tracking data.

mod stream;

pub use stream::StreamOutput;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use owo_colors::OwoColorize;
use serde::Serialize;
use serde_json::{Value as JsonValue, json};

use crate::chart::{Difficulty, SongInfo, UnlockData, get_unlock_state_for_difficulty};
use crate::error::Result;
use crate::play::{PlayData, UnlockType, calculate_dj_points};
use crate::score::{Grade, Lamp, ScoreData, ScoreMap};

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

/// Generate TSV header with all columns
pub fn format_full_tsv_header() -> String {
    let columns = vec![
        "title",
        "difficulty",
        "title2",
        "bpm",
        "artist",
        "genre",
        "notecount",
        "level",
        "playtype",
        "grade",
        "lamp",
        "misscount",
        "exscore",
        "pgreat",
        "great",
        "good",
        "bad",
        "poor",
        "combobreak",
        "fast",
        "slow",
        "style",
        "style2",
        "assist",
        "range",
        "date",
    ];

    columns.join("\t")
}

/// Generate TSV row with all columns
pub fn format_full_tsv_row(play_data: &PlayData) -> String {
    let values: Vec<String> = vec![
        play_data.chart.title.to_string(),
        play_data.chart.difficulty.short_name().to_string(),
        play_data.chart.title_english.to_string(),
        play_data.chart.bpm.to_string(),
        play_data.chart.artist.to_string(),
        play_data.chart.genre.to_string(),
        play_data.chart.total_notes.to_string(),
        play_data.chart.level.to_string(),
        play_data.judge.play_type.short_name().to_string(),
        play_data.grade.short_name().to_string(),
        play_data.lamp.short_name().to_string(),
        if play_data.miss_count_valid() {
            play_data.miss_count().to_string()
        } else {
            "-".to_string()
        },
        play_data.ex_score.to_string(),
        play_data.judge.pgreat.to_string(),
        play_data.judge.great.to_string(),
        play_data.judge.good.to_string(),
        play_data.judge.bad.to_string(),
        play_data.judge.poor.to_string(),
        play_data.judge.combo_break.to_string(),
        play_data.judge.fast.to_string(),
        play_data.judge.slow.to_string(),
        play_data.settings.style.as_str().to_string(),
        play_data
            .settings
            .style2
            .map(|s| s.as_str())
            .unwrap_or("OFF")
            .to_string(),
        play_data.settings.assist.as_str().to_string(),
        play_data.settings.range.as_str().to_string(),
        play_data.timestamp.to_rfc3339(),
    ];

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
            "comboBreak": play_data.judge.combo_break
        }
    });

    if play_data.miss_count_valid() {
        entry["hitMeta"]["bp"] = json!(play_data.miss_count());
    }

    entry
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
    pub song_id: u32,
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
    custom_types: &HashMap<u32, String>,
) -> Result<()> {
    let mut lines = vec![format_tracker_tsv_header()];

    // Get all song IDs from song database (sorted)
    let mut song_ids: Vec<&u32> = song_db.keys().collect();
    song_ids.sort();

    for &song_id in song_ids {
        if let Some(entry) =
            generate_tracker_entry(song_id, song_db, unlock_db, score_map, custom_types)
        {
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
    custom_types: &HashMap<u32, String>,
) -> Option<String> {
    let song = song_db.get(&song_id)?;
    let unlock = unlock_db.get(&song_id)?;
    let scores = score_map.get(song_id);

    let mut columns = Vec::new();

    // Title
    columns.push(song.title.to_string());

    // Type and Label
    let type_name = match unlock.unlock_type {
        UnlockType::Base => "Base",
        UnlockType::Bits => "Bits",
        UnlockType::Sub => "Sub",
    };
    columns.push(type_name.to_string());

    let label = custom_types
        .get(&song_id)
        .cloned()
        .unwrap_or_else(|| type_name.to_string());
    columns.push(label);

    // Bit costs (for N, H, A)
    for i in [1, 2, 3] {
        // SPN, SPH, SPA indices
        let cost = if unlock.unlock_type == UnlockType::Bits && !custom_types.contains_key(&song_id)
        {
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
                crate::play::PlayData::calculate_grade(ex_score, total_notes)
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

/// Format play data for console display with colored output
///
/// Returns a multi-line string with a boxed format.
/// If `personal_best` is provided, shows improvement indicators.
pub fn format_play_data_console(play_data: &PlayData, personal_best: Option<&ScoreData>) -> String {
    let mut output = String::new();

    // Build title line: "冥 [SPA Lv.12]"
    let difficulty_label = format_colored_difficulty(&play_data.chart.difficulty);
    let title_content = format!(
        "  {} [{} Lv.{}]",
        play_data.chart.title.bold(),
        difficulty_label,
        play_data.chart.level
    );

    // Calculate display width (approximate, accounting for ANSI codes)
    let content_width = play_data.chart.title.len()
        + play_data.chart.difficulty.short_name().len()
        + play_data.chart.level.to_string().len()
        + 12; // " [" + " Lv." + "]" + padding
    let border_width = content_width.max(50);

    // Build border line
    let border: String = "━".repeat(border_width);
    let border_dim = border.dimmed();

    // Build option string
    let option = play_data.settings.style.as_str();

    // Compare with personal best
    let comparison = compare_with_personal_best(play_data, personal_best);

    // Build score string with optional diff
    let score_str = match comparison.score_diff {
        Some(diff) => format!(
            "{} ({})",
            play_data.ex_score,
            format!("+{}", diff).green()
        ),
        None => play_data.ex_score.to_string(),
    };

    // Build grade string with optional previous grade
    let grade_str = match comparison.previous_grade {
        Some(prev) => format!(
            "{}→{}",
            format_colored_grade(&prev),
            format_colored_grade(&play_data.grade)
        ),
        None => format_colored_grade(&play_data.grade),
    };

    // Build lamp string with optional previous lamp
    let lamp_str = match comparison.previous_lamp {
        Some(prev) => format!(
            "{}→{}",
            format_colored_lamp(&prev),
            format_colored_lamp(&play_data.lamp)
        ),
        None => format_colored_lamp(&play_data.lamp),
    };

    let line1 = format!(
        "  Option: {}  Score: {} {}  Lamp: {}",
        option, score_str, grade_str, lamp_str
    );

    let judge = &play_data.judge;
    let line2 = format!(
        "  Judge: {}/{}/{}/{}/{}  Fast/Slow: {}/{}  CB: {}",
        judge.pgreat.cyan(),
        judge.great.yellow(),
        judge.good.truecolor(255, 200, 0), // gold (between yellow and orange)
        judge.bad.truecolor(255, 165, 0),  // orange
        judge.poor.red(),
        judge.fast.blue(),
        judge.slow.red(),
        judge.combo_break
    );

    let _ = writeln!(output, "{}", border_dim);
    let _ = writeln!(output, "{}", title_content);
    let _ = writeln!(output, "{}", border_dim);
    let _ = writeln!(output, "{}", line1);
    let _ = writeln!(output, "{}", line2);
    let _ = write!(output, "{}", border_dim);

    output
}

/// Format difficulty with color
fn format_colored_difficulty(difficulty: &Difficulty) -> String {
    let name = difficulty.short_name();
    match difficulty.expand_name() {
        "BEGINNER" => name.green().to_string(),
        "NORMAL" => name.blue().to_string(),
        "HYPER" => name.yellow().to_string(),
        "ANOTHER" => name.red().to_string(),
        "LEGGENDARIA" => name.purple().to_string(),
        _ => name.to_string(),
    }
}

/// Format lamp with color
fn format_colored_lamp(lamp: &Lamp) -> String {
    let name = lamp.short_name();
    match lamp {
        Lamp::NoPlay => name.dimmed().to_string(),
        Lamp::Failed => name.red().to_string(),
        Lamp::AssistClear => name.purple().to_string(),
        Lamp::EasyClear => name.green().to_string(),
        Lamp::Clear => name.blue().to_string(),
        Lamp::HardClear => name.bold().to_string(),
        Lamp::ExHardClear => name.yellow().to_string(),
        Lamp::FullCombo | Lamp::Pfc => name.cyan().to_string(),
    }
}

/// Format grade with color
fn format_colored_grade(grade: &Grade) -> String {
    let name = grade.short_name();
    match grade {
        Grade::NoPlay => name.dimmed().to_string(),
        // F～B: blue to pale cyan (near white) gradient
        Grade::F => name.truecolor(0, 0, 255).to_string(),
        Grade::E => name.truecolor(50, 100, 255).to_string(),
        Grade::D => name.truecolor(110, 170, 255).to_string(),
        Grade::C => name.truecolor(170, 215, 255).to_string(),
        Grade::B => name.truecolor(220, 245, 255).to_string(),
        // A: cyan
        Grade::A => name.truecolor(0, 255, 255).to_string(),
        // AA: silver
        Grade::Aa => name.truecolor(192, 192, 192).to_string(),
        // AAA: gold
        Grade::Aaa => name.truecolor(255, 200, 0).bold().to_string(),
    }
}

/// Personal best comparison result
#[derive(Debug, Clone, Default)]
pub struct PersonalBestComparison {
    /// Score difference (positive = improvement)
    pub score_diff: Option<i32>,
    /// Previous grade if improved
    pub previous_grade: Option<Grade>,
    /// Previous lamp if improved
    pub previous_lamp: Option<Lamp>,
}

/// Compare current play data with personal best
pub fn compare_with_personal_best(
    play_data: &PlayData,
    best: Option<&ScoreData>,
) -> PersonalBestComparison {
    let Some(best) = best else {
        return PersonalBestComparison::default();
    };

    let diff_index = play_data.chart.difficulty as usize;
    let best_score = best.score[diff_index];
    let best_lamp = best.lamp[diff_index];

    let mut comparison = PersonalBestComparison::default();

    // Score comparison: only show diff if best score exists and current is higher
    if best_score > 0 && play_data.ex_score > best_score {
        comparison.score_diff = Some(play_data.ex_score as i32 - best_score as i32);
    }

    // Grade comparison: calculate grade from best score and compare
    if best_score > 0 && play_data.chart.total_notes > 0 {
        let best_grade = PlayData::calculate_grade(best_score, play_data.chart.total_notes);
        if play_data.grade > best_grade {
            comparison.previous_grade = Some(best_grade);
        }
    }

    // Lamp comparison: direct comparison (Lamp implements Ord)
    if best_lamp != Lamp::NoPlay && play_data.lamp > best_lamp {
        comparison.previous_lamp = Some(best_lamp);
    }

    comparison
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
        if play_data.data_available {
            ""
        } else {
            "[INVALID]"
        }
    )
}

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
            let grade = crate::play::PlayData::calculate_grade(ex_score, total_notes);
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
    custom_types: &HashMap<u32, String>,
) -> String {
    let mut lines = vec![format_tracker_tsv_header()];

    // Get all song IDs from song database (sorted)
    let mut song_ids: Vec<&u32> = song_db.keys().collect();
    song_ids.sort();

    for &song_id in song_ids {
        if let Some(entry) =
            generate_tracker_entry(song_id, song_db, unlock_db, score_map, custom_types)
        {
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
    fn test_format_tsv_header() {
        let header = format_tsv_header();
        assert!(header.contains("Timestamp"));
        assert!(header.contains("Title"));
        assert!(header.contains("Difficulty"));
        assert!(header.contains("EX Score"));
        assert!(header.contains("Lamp"));
    }

    #[test]
    fn test_format_full_tsv_header() {
        let header = format_full_tsv_header();
        assert!(header.contains("title"));
        assert!(header.contains("difficulty"));
        assert!(header.contains("notecount"));
        assert!(header.contains("exscore"));
        assert!(header.contains("date"));
    }

    #[test]
    fn test_format_tracker_tsv_header() {
        let header = format_tracker_tsv_header();
        assert!(header.contains("Title"));
        assert!(header.contains("Type"));
        assert!(header.contains("Label"));
        assert!(header.contains("SP DJ Points"));
        assert!(header.contains("DP DJ Points"));
        assert!(header.contains("SPA Lamp"));
        assert!(header.contains("DPA Lamp"));
    }

    #[test]
    fn test_tsv_row_data() {
        let data = TsvRowData {
            timestamp: "2025-01-30T12:00:00Z",
            title: "Test Song",
            difficulty: "SPA",
            level: 12,
            ex_score: 2500,
            grade: "AAA",
            lamp: "HARD",
            pgreat: 1200,
            great: 100,
            good: 5,
            bad: 2,
            poor: 1,
            fast: 30,
            slow: 20,
            combo_break: 3,
        };
        let row = format_tsv_row(&data);

        assert!(row.contains("Test Song"));
        assert!(row.contains("SPA"));
        assert!(row.contains("2500"));
        assert!(row.contains("AAA"));
        assert!(row.contains("HARD"));
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
        let custom_types: HashMap<u32, String> = HashMap::new();

        let tsv = generate_tracker_tsv(&song_db, &unlock_db, &score_map, &custom_types);
        let lines: Vec<&str> = tsv.lines().collect();

        // Should only have header
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Title"));
    }

    #[test]
    fn test_format_play_summary() {
        use crate::chart::ChartInfo;
        use crate::play::{PlayType, Settings};
        use crate::score::Judge;

        let play_data = PlayData {
            chart: ChartInfo {
                song_id: 1000,
                title: Arc::from("Test Song"),
                title_english: Arc::from(""),
                artist: Arc::from(""),
                genre: Arc::from(""),
                bpm: Arc::from("150"),
                difficulty: Difficulty::SpA,
                level: 12,
                total_notes: 1000,
                unlocked: true,
            },
            judge: Judge {
                play_type: PlayType::P1,
                pgreat: 900,
                great: 100,
                good: 0,
                bad: 0,
                poor: 0,
                fast: 30,
                slow: 20,
                combo_break: 0,
                premature_end: false,
            },
            settings: Settings::default(),
            ex_score: 1900,
            lamp: Lamp::FullCombo,
            grade: Grade::Aaa,
            data_available: true,
            timestamp: chrono::Utc::now(),
        };

        let summary = format_play_summary(&play_data);
        assert!(summary.contains("Test Song"));
        assert!(summary.contains("SPA"));
        assert!(summary.contains("FC"));
        assert!(summary.contains("AAA"));
        assert!(summary.contains("1900"));
        assert!(!summary.contains("INVALID"));
    }

    fn create_test_play_data(ex_score: u32, grade: Grade, lamp: Lamp) -> PlayData {
        use crate::chart::ChartInfo;
        use crate::play::{PlayType, Settings};
        use crate::score::Judge;

        PlayData {
            chart: ChartInfo {
                song_id: 1000,
                title: Arc::from("Test Song"),
                title_english: Arc::from(""),
                artist: Arc::from(""),
                genre: Arc::from(""),
                bpm: Arc::from("150"),
                difficulty: Difficulty::SpA,
                level: 12,
                total_notes: 1000, // max EX = 2000
                unlocked: true,
            },
            judge: Judge {
                play_type: PlayType::P1,
                pgreat: 900,
                great: 100,
                good: 0,
                bad: 0,
                poor: 0,
                fast: 30,
                slow: 20,
                combo_break: 0,
                premature_end: false,
            },
            settings: Settings::default(),
            ex_score,
            lamp,
            grade,
            data_available: true,
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_compare_with_personal_best_no_best() {
        let play_data = create_test_play_data(1800, Grade::Aaa, Lamp::HardClear);
        let comparison = compare_with_personal_best(&play_data, None);

        assert!(comparison.score_diff.is_none());
        assert!(comparison.previous_grade.is_none());
        assert!(comparison.previous_lamp.is_none());
    }

    #[test]
    fn test_compare_with_personal_best_score_improvement() {
        // Current: AAA (1800), Best: AAA (1780) - same grade, score up
        let play_data = create_test_play_data(1800, Grade::Aaa, Lamp::HardClear);

        let mut best = ScoreData::new(1000);
        best.score[Difficulty::SpA as usize] = 1780; // Also AAA
        best.lamp[Difficulty::SpA as usize] = Lamp::HardClear;

        let comparison = compare_with_personal_best(&play_data, Some(&best));

        assert_eq!(comparison.score_diff, Some(20));
        assert!(comparison.previous_grade.is_none()); // Both AAA
        assert!(comparison.previous_lamp.is_none()); // Same lamp
    }

    #[test]
    fn test_compare_with_personal_best_grade_improvement() {
        // Current: AAA (1778+), Best: AA (1556-1777)
        let play_data = create_test_play_data(1800, Grade::Aaa, Lamp::Clear);

        let mut best = ScoreData::new(1000);
        best.score[Difficulty::SpA as usize] = 1600; // AA
        best.lamp[Difficulty::SpA as usize] = Lamp::Clear;

        let comparison = compare_with_personal_best(&play_data, Some(&best));

        assert_eq!(comparison.score_diff, Some(200));
        assert_eq!(comparison.previous_grade, Some(Grade::Aa));
        assert!(comparison.previous_lamp.is_none());
    }

    #[test]
    fn test_compare_with_personal_best_lamp_improvement() {
        let play_data = create_test_play_data(1800, Grade::Aaa, Lamp::HardClear);

        let mut best = ScoreData::new(1000);
        best.score[Difficulty::SpA as usize] = 1800; // Same score
        best.lamp[Difficulty::SpA as usize] = Lamp::Clear;

        let comparison = compare_with_personal_best(&play_data, Some(&best));

        assert!(comparison.score_diff.is_none()); // Same score
        assert!(comparison.previous_grade.is_none()); // Both AAA
        assert_eq!(comparison.previous_lamp, Some(Lamp::Clear));
    }

    #[test]
    fn test_compare_with_personal_best_no_improvement() {
        let play_data = create_test_play_data(1600, Grade::Aa, Lamp::Clear);

        let mut best = ScoreData::new(1000);
        best.score[Difficulty::SpA as usize] = 1800; // Better
        best.lamp[Difficulty::SpA as usize] = Lamp::HardClear; // Better

        let comparison = compare_with_personal_best(&play_data, Some(&best));

        assert!(comparison.score_diff.is_none());
        assert!(comparison.previous_grade.is_none());
        assert!(comparison.previous_lamp.is_none());
    }

    #[test]
    fn test_compare_with_personal_best_first_clear() {
        // First clear: best lamp is NoPlay
        let play_data = create_test_play_data(1800, Grade::Aaa, Lamp::HardClear);

        let mut best = ScoreData::new(1000);
        best.score[Difficulty::SpA as usize] = 0;
        best.lamp[Difficulty::SpA as usize] = Lamp::NoPlay;

        let comparison = compare_with_personal_best(&play_data, Some(&best));

        // NoPlay to something is not shown as lamp improvement
        assert!(comparison.score_diff.is_none()); // No previous score
        assert!(comparison.previous_grade.is_none()); // No previous grade
        assert!(comparison.previous_lamp.is_none()); // NoPlay is not shown
    }
}
