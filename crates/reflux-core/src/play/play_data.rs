use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::chart::ChartInfo;
use crate::play::{AssistType, Settings};
use crate::score::{Grade, Judge, Lamp};

/// Complete play data for a single play
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayData {
    pub timestamp: DateTime<Utc>,
    pub chart: ChartInfo,
    pub ex_score: u32,
    pub grade: Grade,
    pub lamp: Lamp,
    pub judge: Judge,
    pub settings: Settings,
    /// False if play data isn't available (H-RAN, BATTLE or assist options enabled)
    pub data_available: bool,
}

impl PlayData {
    /// Check if miss count should be saved
    /// (not available when using assist options or premature end)
    pub fn miss_count_valid(&self) -> bool {
        self.data_available && !self.judge.premature_end && self.settings.assist == AssistType::Off
    }

    /// Get miss count (bad + poor)
    pub fn miss_count(&self) -> u32 {
        self.judge.miss_count()
    }

    /// Calculate grade from EX score
    pub fn calculate_grade(ex_score: u32, total_notes: u32) -> Grade {
        if total_notes == 0 {
            return Grade::F;
        }
        let max_ex = total_notes * 2;
        let ratio = ex_score as f64 / max_ex as f64;
        Grade::from_score_ratio(ratio)
    }

    /// Upgrade lamp to PFC if applicable
    pub fn upgrade_lamp_if_pfc(&mut self) {
        if self.judge.is_pfc() && self.lamp == Lamp::FullCombo {
            self.lamp = Lamp::Pfc;
        }
    }
}

// DJ Points calculation constants
// Based on the official DJ Points formula from beatmania IIDX
const DJ_POINTS_GRADE_A_BASE_BONUS: i32 = 10;
const DJ_POINTS_GRADE_BONUS_PER_RANK: i32 = 5;
const DJ_POINTS_LAMP_BONUS_PER_RANK: i32 = 5;
const DJ_POINTS_LAMP_HARD_CLEAR_BONUS: i32 = 5;
const DJ_POINTS_BASE_MULTIPLIER: i32 = 100;
const DJ_POINTS_DIVISOR: f64 = 10000.0;

/// Calculate DJ Points for a given score and lamp
///
/// Formula:
/// - C = (grade >= A ? 10 : 0) + max(0, grade - A) * 5
/// - L = max(0, lamp - AC) * 5 + (lamp >= HC ? 5 : 0)
/// - DJ Points = score * (100 + C + L) / 10000
pub fn calculate_dj_points(ex_score: u32, grade: Grade, lamp: Lamp) -> f64 {
    // Grade bonus: A=10, AA=15, AAA=20
    let grade_val = grade as i32;
    let grade_a_val = Grade::A as i32;
    let grade_bonus = if grade_val >= grade_a_val {
        DJ_POINTS_GRADE_A_BASE_BONUS
            + (grade_val - grade_a_val).max(0) * DJ_POINTS_GRADE_BONUS_PER_RANK
    } else {
        0
    };

    // Lamp bonus: NC/EC=5, HC=15, EX=20, FC=25, PFC=30
    let lamp_val = lamp as i32;
    let lamp_ac_val = Lamp::AssistClear as i32;
    let lamp_hc_val = Lamp::HardClear as i32;
    let lamp_bonus = (lamp_val - lamp_ac_val).max(0) * DJ_POINTS_LAMP_BONUS_PER_RANK
        + if lamp_val >= lamp_hc_val {
            DJ_POINTS_LAMP_HARD_CLEAR_BONUS
        } else {
            0
        };

    // DJ Points calculation
    ex_score as f64 * (DJ_POINTS_BASE_MULTIPLIER + grade_bonus + lamp_bonus) as f64
        / DJ_POINTS_DIVISOR
}

/// Calculate DJ Points from score and total notes
pub fn calculate_dj_points_from_score(ex_score: u32, total_notes: u32, lamp: Lamp) -> f64 {
    if total_notes == 0 {
        return 0.0;
    }

    let grade = PlayData::calculate_grade(ex_score, total_notes);
    calculate_dj_points(ex_score, grade, lamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_dj_points() {
        // AAA + PFC should give maximum bonus
        let djp = calculate_dj_points(2000, Grade::Aaa, Lamp::Pfc);
        // C = 10 + 2*5 = 20, L = 6*5 + 5 = 35
        // DJ Points = 2000 * (100 + 20 + 35) / 10000 = 2000 * 155 / 10000 = 31.0
        assert!((djp - 31.0).abs() < 0.01);

        // A + Clear
        let djp = calculate_dj_points(1000, Grade::A, Lamp::Clear);
        // C = 10, L = 2*5 + 0 = 10
        // DJ Points = 1000 * (100 + 10 + 10) / 10000 = 1000 * 120 / 10000 = 12.0
        assert!((djp - 12.0).abs() < 0.01);

        // B + Failed
        let djp = calculate_dj_points(500, Grade::B, Lamp::Failed);
        // C = 0, L = 0
        // DJ Points = 500 * 100 / 10000 = 5.0
        assert!((djp - 5.0).abs() < 0.01);
    }
}
