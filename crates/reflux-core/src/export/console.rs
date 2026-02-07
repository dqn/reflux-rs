//! Console output formatting with colored display

use std::fmt::Write as _;

use owo_colors::OwoColorize;

use crate::chart::Difficulty;
use crate::play::PlayData;
use crate::score::{Grade, Lamp, ScoreData};

use super::comparison::compare_with_personal_best;

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
        Some(diff) => format!("{} ({})", play_data.ex_score, format!("+{}", diff).green()),
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

    let judge = &play_data.judge;

    let _ = writeln!(output, "{}", border_dim);
    let _ = writeln!(output, "{}", title_content);
    let _ = writeln!(output, "{}", border_dim);
    let _ = writeln!(output, "  OPTION : {}", option);
    let _ = writeln!(output, "  LAMP   : {}", lamp_str);
    let _ = writeln!(output, "  SCORE  : {} {}", score_str, grade_str);
    if play_data.miss_count_valid() {
        let miss = play_data.miss_count();
        match comparison.miss_count_diff {
            Some(diff) => {
                let _ = writeln!(
                    output,
                    "  MISS   : {} ({})",
                    miss,
                    format!("{}", diff).green()
                );
            }
            None => {
                let _ = writeln!(output, "  MISS   : {}", miss);
            }
        }
    } else {
        let _ = writeln!(output, "  MISS   : -");
    }
    let _ = writeln!(
        output,
        "  JUDGE  : {}/{}/{}/{}/{}",
        judge.pgreat.cyan(),
        judge.great.truecolor(255, 200, 0),
        judge.good.truecolor(255, 165, 0),
        judge.bad.truecolor(230, 120, 0),
        judge.poor.truecolor(200, 50, 30),
    );
    let _ = writeln!(
        output,
        "  F/S    : {}/{}",
        judge.fast.blue(),
        judge.slow.red()
    );
    let _ = writeln!(output, "  CB     : {}", judge.combo_break);
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
        Lamp::Clear => name.cyan().to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::chart::ChartInfo;
    use crate::play::{PlayType, Settings};
    use crate::score::Judge;

    #[test]
    fn test_format_play_summary() {
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
}
