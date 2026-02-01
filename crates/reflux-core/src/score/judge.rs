use serde::{Deserialize, Serialize};

use crate::play::PlayType;

/// Raw judge data for a single player side (P1 or P2)
#[derive(Debug, Clone, Default)]
pub struct PlayerJudge {
    pub pgreat: u32,
    pub great: u32,
    pub good: u32,
    pub bad: u32,
    pub poor: u32,
    pub combo_break: u32,
    pub fast: u32,
    pub slow: u32,
    pub measure_end: u32,
}

impl PlayerJudge {
    /// Calculate total note count for this side
    pub fn total_notes(&self) -> u32 {
        self.pgreat + self.great + self.good + self.bad + self.poor
    }
}

/// Raw judge data from memory (P1 and P2 combined)
#[derive(Debug, Clone, Default)]
pub struct RawJudgeData {
    pub p1: PlayerJudge,
    pub p2: PlayerJudge,
}

/// Judge information from a play
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Judge {
    pub play_type: PlayType,
    pub pgreat: u32,
    pub great: u32,
    pub good: u32,
    pub bad: u32,
    pub poor: u32,
    pub fast: u32,
    pub slow: u32,
    pub combo_break: u32,
    pub premature_end: bool,
}

impl Judge {
    /// Memory layout offsets (each value is 4 bytes)
    /// P1: 0-4 (pgreat, great, good, bad, poor)
    /// P2: 5-9 (pgreat, great, good, bad, poor)
    /// CB: 10-11 (p1, p2)
    /// Fast: 12-13 (p1, p2)
    /// Slow: 14-15 (p1, p2)
    /// Measure end: 16-17 (p1, p2)
    pub const WORD_SIZE: u64 = 4;

    /// Check if this is a Perfect Full Combo (no good/bad/poor)
    pub fn is_pfc(&self) -> bool {
        self.good == 0 && self.bad == 0 && self.poor == 0
    }

    /// Calculate EX score (pgreat * 2 + great)
    pub fn ex_score(&self) -> u32 {
        self.pgreat * 2 + self.great
    }

    /// Calculate miss count (bad + poor)
    pub fn miss_count(&self) -> u32 {
        self.bad + self.poor
    }

    /// Build judge data from raw memory data
    pub fn from_raw_data(raw: RawJudgeData) -> Self {
        let p1_total = raw.p1.total_notes();
        let p2_total = raw.p2.total_notes();

        let play_type = if p1_total == 0 && p2_total > 0 {
            PlayType::P2
        } else if p1_total > 0 && p2_total > 0 {
            PlayType::Dp
        } else {
            PlayType::P1
        };

        Self {
            play_type,
            pgreat: raw.p1.pgreat + raw.p2.pgreat,
            great: raw.p1.great + raw.p2.great,
            good: raw.p1.good + raw.p2.good,
            bad: raw.p1.bad + raw.p2.bad,
            poor: raw.p1.poor + raw.p2.poor,
            fast: raw.p1.fast + raw.p2.fast,
            slow: raw.p1.slow + raw.p2.slow,
            combo_break: raw.p1.combo_break + raw.p2.combo_break,
            premature_end: (raw.p1.measure_end + raw.p2.measure_end) != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_judge_total_notes() {
        let pj = PlayerJudge {
            pgreat: 100,
            great: 50,
            good: 10,
            bad: 5,
            poor: 2,
            ..Default::default()
        };
        assert_eq!(pj.total_notes(), 167);
    }

    #[test]
    fn test_judge_is_pfc() {
        let pfc = Judge {
            pgreat: 1000,
            great: 100,
            good: 0,
            bad: 0,
            poor: 0,
            ..Default::default()
        };
        assert!(pfc.is_pfc());

        let not_pfc = Judge {
            pgreat: 1000,
            great: 100,
            good: 1,
            bad: 0,
            poor: 0,
            ..Default::default()
        };
        assert!(!not_pfc.is_pfc());
    }

    #[test]
    fn test_judge_ex_score() {
        let judge = Judge {
            pgreat: 500,
            great: 100,
            ..Default::default()
        };
        assert_eq!(judge.ex_score(), 1100); // 500*2 + 100
    }

    #[test]
    fn test_judge_miss_count() {
        let judge = Judge {
            bad: 5,
            poor: 3,
            ..Default::default()
        };
        assert_eq!(judge.miss_count(), 8);
    }

    #[test]
    fn test_from_raw_data_p1() {
        let raw = RawJudgeData {
            p1: PlayerJudge {
                pgreat: 100,
                great: 50,
                ..Default::default()
            },
            p2: PlayerJudge::default(),
        };
        let judge = Judge::from_raw_data(raw);
        assert_eq!(judge.play_type, PlayType::P1);
        assert_eq!(judge.pgreat, 100);
        assert_eq!(judge.great, 50);
    }

    #[test]
    fn test_from_raw_data_p2() {
        let raw = RawJudgeData {
            p1: PlayerJudge::default(),
            p2: PlayerJudge {
                pgreat: 200,
                great: 100,
                ..Default::default()
            },
        };
        let judge = Judge::from_raw_data(raw);
        assert_eq!(judge.play_type, PlayType::P2);
        assert_eq!(judge.pgreat, 200);
        assert_eq!(judge.great, 100);
    }

    #[test]
    fn test_from_raw_data_dp() {
        let raw = RawJudgeData {
            p1: PlayerJudge {
                pgreat: 100,
                great: 50,
                ..Default::default()
            },
            p2: PlayerJudge {
                pgreat: 100,
                great: 50,
                ..Default::default()
            },
        };
        let judge = Judge::from_raw_data(raw);
        assert_eq!(judge.play_type, PlayType::Dp);
        assert_eq!(judge.pgreat, 200);
        assert_eq!(judge.great, 100);
    }

    #[test]
    fn test_from_raw_data_premature_end() {
        let raw = RawJudgeData {
            p1: PlayerJudge {
                pgreat: 100,
                measure_end: 1,
                ..Default::default()
            },
            p2: PlayerJudge::default(),
        };
        let judge = Judge::from_raw_data(raw);
        assert!(judge.premature_end);

        let raw_no_end = RawJudgeData {
            p1: PlayerJudge {
                pgreat: 100,
                measure_end: 0,
                ..Default::default()
            },
            p2: PlayerJudge::default(),
        };
        let judge_no_end = Judge::from_raw_data(raw_no_end);
        assert!(!judge_no_end.premature_end);
    }

    #[test]
    fn test_from_raw_data_combines_values() {
        let raw = RawJudgeData {
            p1: PlayerJudge {
                pgreat: 100,
                fast: 10,
                slow: 5,
                combo_break: 2,
                ..Default::default()
            },
            p2: PlayerJudge {
                pgreat: 50,
                fast: 8,
                slow: 3,
                combo_break: 1,
                ..Default::default()
            },
        };
        let judge = Judge::from_raw_data(raw);
        assert_eq!(judge.pgreat, 150);
        assert_eq!(judge.fast, 18);
        assert_eq!(judge.slow, 8);
        assert_eq!(judge.combo_break, 3);
    }
}
