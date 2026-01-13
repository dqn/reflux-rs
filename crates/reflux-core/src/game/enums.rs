use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Difficulty {
    SpB = 0,
    SpN = 1,
    SpH = 2,
    SpA = 3,
    SpL = 4,
    DpB = 5,
    DpN = 6,
    DpH = 7,
    DpA = 8,
    DpL = 9,
}

impl Difficulty {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::SpB),
            1 => Some(Self::SpN),
            2 => Some(Self::SpH),
            3 => Some(Self::SpA),
            4 => Some(Self::SpL),
            5 => Some(Self::DpB),
            6 => Some(Self::DpN),
            7 => Some(Self::DpH),
            8 => Some(Self::DpA),
            9 => Some(Self::DpL),
            _ => None,
        }
    }

    pub fn is_sp(&self) -> bool {
        matches!(
            self,
            Self::SpB | Self::SpN | Self::SpH | Self::SpA | Self::SpL
        )
    }

    pub fn is_dp(&self) -> bool {
        !self.is_sp()
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::SpB => "SPB",
            Self::SpN => "SPN",
            Self::SpH => "SPH",
            Self::SpA => "SPA",
            Self::SpL => "SPL",
            Self::DpB => "DPB",
            Self::DpN => "DPN",
            Self::DpH => "DPH",
            Self::DpA => "DPA",
            Self::DpL => "DPL",
        }
    }

    /// Get the expanded difficulty name (e.g., "NORMAL", "HYPER")
    pub fn expand_name(&self) -> &'static str {
        match self {
            Self::SpB => "BEGINNER",
            Self::DpB => "BEGINNER",
            Self::SpN | Self::DpN => "NORMAL",
            Self::SpH | Self::DpH => "HYPER",
            Self::SpA | Self::DpA => "ANOTHER",
            Self::SpL | Self::DpL => "LEGGENDARIA",
        }
    }

    /// Get the color code for difficulty (for OBS output)
    pub fn color_code(&self) -> &'static str {
        match self {
            Self::SpB | Self::DpB => "#32CD32", // Green for beginner
            Self::SpN | Self::DpN => "#0FABFD", // Blue for normal
            Self::SpH | Self::DpH => "#F4903C", // Orange for hyper
            Self::SpA | Self::DpA => "#E52B19", // Red for another
            Self::SpL | Self::DpL => "#9B30FF", // Purple for leggendaria
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[repr(u8)]
pub enum Lamp {
    #[default]
    NoPlay = 0,
    Failed = 1,
    AssistClear = 2,
    EasyClear = 3,
    Clear = 4,
    HardClear = 5,
    ExHardClear = 6,
    FullCombo = 7,
    Pfc = 8,
}

impl Lamp {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::NoPlay),
            1 => Some(Self::Failed),
            2 => Some(Self::AssistClear),
            3 => Some(Self::EasyClear),
            4 => Some(Self::Clear),
            5 => Some(Self::HardClear),
            6 => Some(Self::ExHardClear),
            7 => Some(Self::FullCombo),
            8 => Some(Self::Pfc),
            _ => None,
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::NoPlay => "NO PLAY",
            Self::Failed => "FAILED",
            Self::AssistClear => "ASSIST",
            Self::EasyClear => "EASY",
            Self::Clear => "CLEAR",
            Self::HardClear => "HARD",
            Self::ExHardClear => "EX HARD",
            Self::FullCombo => "FC",
            Self::Pfc => "PFC",
        }
    }

    /// Get the expanded lamp name (for display and export)
    pub fn expand_name(&self) -> &'static str {
        match self {
            Self::NoPlay => "NO PLAY",
            Self::Failed => "FAILED",
            Self::AssistClear => "ASSIST CLEAR",
            Self::EasyClear => "EASY CLEAR",
            Self::Clear => "CLEAR",
            Self::HardClear => "HARD CLEAR",
            Self::ExHardClear => "EX HARD CLEAR",
            Self::FullCombo | Self::Pfc => "FULL COMBO",
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[repr(u8)]
pub enum Grade {
    #[default]
    NoPlay = 0,
    F = 1,
    E = 2,
    D = 3,
    C = 4,
    B = 5,
    A = 6,
    Aa = 7,
    Aaa = 8,
}

impl Grade {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::NoPlay),
            1 => Some(Self::F),
            2 => Some(Self::E),
            3 => Some(Self::D),
            4 => Some(Self::C),
            5 => Some(Self::B),
            6 => Some(Self::A),
            7 => Some(Self::Aa),
            8 => Some(Self::Aaa),
            _ => None,
        }
    }

    pub fn from_score_ratio(ratio: f64) -> Self {
        if ratio >= 8.0 / 9.0 {
            Self::Aaa
        } else if ratio >= 7.0 / 9.0 {
            Self::Aa
        } else if ratio >= 6.0 / 9.0 {
            Self::A
        } else if ratio >= 5.0 / 9.0 {
            Self::B
        } else if ratio >= 4.0 / 9.0 {
            Self::C
        } else if ratio >= 3.0 / 9.0 {
            Self::D
        } else if ratio >= 2.0 / 9.0 {
            Self::E
        } else {
            Self::F
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::NoPlay => "-",
            Self::F => "F",
            Self::E => "E",
            Self::D => "D",
            Self::C => "C",
            Self::B => "B",
            Self::A => "A",
            Self::Aa => "AA",
            Self::Aaa => "AAA",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PlayType {
    #[default]
    P1,
    P2,
    Dp,
}

impl PlayType {
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::P1 => "1P",
            Self::P2 => "2P",
            Self::Dp => "DP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum UnlockType {
    #[default]
    Base,
    Bits,
    Sub,
}

impl UnlockType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Base),
            1 => Some(Self::Bits),
            2 => Some(Self::Sub),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Unknown,
    SongSelect,
    Playing,
    ResultScreen,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_difficulty_from_u8() {
        assert_eq!(Difficulty::from_u8(0), Some(Difficulty::SpB));
        assert_eq!(Difficulty::from_u8(4), Some(Difficulty::SpL));
        assert_eq!(Difficulty::from_u8(5), Some(Difficulty::DpB));
        assert_eq!(Difficulty::from_u8(9), Some(Difficulty::DpL));
        assert_eq!(Difficulty::from_u8(10), None);
    }

    #[test]
    fn test_difficulty_is_sp_dp() {
        assert!(Difficulty::SpN.is_sp());
        assert!(!Difficulty::SpN.is_dp());
        assert!(Difficulty::DpA.is_dp());
        assert!(!Difficulty::DpA.is_sp());
    }

    #[test]
    fn test_grade_from_score_ratio() {
        assert_eq!(Grade::from_score_ratio(1.0), Grade::Aaa);
        assert_eq!(Grade::from_score_ratio(0.9), Grade::Aaa);
        assert_eq!(Grade::from_score_ratio(8.0 / 9.0), Grade::Aaa);
        assert_eq!(Grade::from_score_ratio(7.0 / 9.0), Grade::Aa);
        assert_eq!(Grade::from_score_ratio(6.0 / 9.0), Grade::A);
        assert_eq!(Grade::from_score_ratio(5.0 / 9.0), Grade::B);
        assert_eq!(Grade::from_score_ratio(4.0 / 9.0), Grade::C);
        assert_eq!(Grade::from_score_ratio(3.0 / 9.0), Grade::D);
        assert_eq!(Grade::from_score_ratio(2.0 / 9.0), Grade::E);
        assert_eq!(Grade::from_score_ratio(0.1), Grade::F);
    }

    #[test]
    fn test_lamp_ordering() {
        assert!(Lamp::Pfc > Lamp::FullCombo);
        assert!(Lamp::FullCombo > Lamp::ExHardClear);
        assert!(Lamp::Failed < Lamp::Clear);
    }
}
