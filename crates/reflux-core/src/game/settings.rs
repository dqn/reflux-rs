use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::game::PlayType;

/// Error for invalid enum value conversion
#[derive(Debug, Error)]
#[error("Invalid {type_name} value: {value}")]
pub struct InvalidEnumValueError {
    type_name: &'static str,
    value: i32,
}

impl InvalidEnumValueError {
    pub fn new(type_name: &'static str, value: i32) -> Self {
        Self { type_name, value }
    }
}

/// Play settings (options selected before playing)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    pub style: Style,
    pub style2: Option<Style>, // For DP second side
    pub gauge: GaugeType,
    pub assist: AssistType,
    pub range: RangeType,
    pub flip: bool,
    pub battle: bool,
    pub h_ran: bool,
}

impl Settings {
    /// P2 settings offset (4 * 15 = 60 bytes)
    pub const P2_OFFSET: u64 = 60;
    pub const WORD_SIZE: u64 = 4;

    /// Build settings from raw memory values
    #[allow(clippy::too_many_arguments)] // Mapping raw memory layout requires many parameters
    pub fn from_raw_values(
        play_type: PlayType,
        style_val: i32,
        style2_val: i32,
        gauge_val: i32,
        assist_val: i32,
        range_val: i32,
        flip_val: i32,
        battle_val: i32,
        h_ran_val: i32,
    ) -> Self {
        Self {
            style: style_val.try_into().unwrap_or_default(),
            style2: if play_type == PlayType::Dp {
                Some(style2_val.try_into().unwrap_or_default())
            } else {
                None
            },
            gauge: gauge_val.try_into().unwrap_or_default(),
            assist: assist_val.try_into().unwrap_or_default(),
            range: range_val.try_into().unwrap_or_default(),
            flip: flip_val == 1,
            battle: battle_val == 1,
            h_ran: h_ran_val == 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Style {
    #[default]
    Off,
    Random,
    RRandom,
    SRandom,
    Mirror,
    SynchronizeRandom,
    SymmetryRandom,
}

impl TryFrom<i32> for Style {
    type Error = InvalidEnumValueError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Off),
            1 => Ok(Self::Random),
            2 => Ok(Self::RRandom),
            3 => Ok(Self::SRandom),
            4 => Ok(Self::Mirror),
            5 => Ok(Self::SynchronizeRandom),
            6 => Ok(Self::SymmetryRandom),
            _ => Err(InvalidEnumValueError::new("Style", value)),
        }
    }
}

impl Style {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Random => "RANDOM",
            Self::RRandom => "R-RANDOM",
            Self::SRandom => "S-RANDOM",
            Self::Mirror => "MIRROR",
            Self::SynchronizeRandom => "SYNCHRONIZE RANDOM",
            Self::SymmetryRandom => "SYMMETRY RANDOM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GaugeType {
    #[default]
    Off,
    AssistEasy,
    Easy,
    Hard,
    ExHard,
}

impl TryFrom<i32> for GaugeType {
    type Error = InvalidEnumValueError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Off),
            1 => Ok(Self::AssistEasy),
            2 => Ok(Self::Easy),
            3 => Ok(Self::Hard),
            4 => Ok(Self::ExHard),
            _ => Err(InvalidEnumValueError::new("GaugeType", value)),
        }
    }
}

impl GaugeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::AssistEasy => "ASSIST EASY",
            Self::Easy => "EASY",
            Self::Hard => "HARD",
            Self::ExHard => "EX HARD",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AssistType {
    #[default]
    Off,
    AutoScratch,
    FiveKeys,
    LegacyNote,
    KeyAssist,
    AnyKey,
}

impl TryFrom<i32> for AssistType {
    type Error = InvalidEnumValueError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Off),
            1 => Ok(Self::AutoScratch),
            2 => Ok(Self::FiveKeys),
            3 => Ok(Self::LegacyNote),
            4 => Ok(Self::KeyAssist),
            5 => Ok(Self::AnyKey),
            _ => Err(InvalidEnumValueError::new("AssistType", value)),
        }
    }
}

impl AssistType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::AutoScratch => "AUTO SCRATCH",
            Self::FiveKeys => "5KEYS",
            Self::LegacyNote => "LEGACY NOTE",
            Self::KeyAssist => "KEY ASSIST",
            Self::AnyKey => "ANY KEY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RangeType {
    #[default]
    Off,
    SuddenPlus,
    HiddenPlus,
    SudHid,
    Lift,
    LiftSud,
}

impl TryFrom<i32> for RangeType {
    type Error = InvalidEnumValueError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Off),
            1 => Ok(Self::SuddenPlus),
            2 => Ok(Self::HiddenPlus),
            3 => Ok(Self::SudHid),
            4 => Ok(Self::Lift),
            5 => Ok(Self::LiftSud),
            _ => Err(InvalidEnumValueError::new("RangeType", value)),
        }
    }
}

impl RangeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::SuddenPlus => "SUDDEN+",
            Self::HiddenPlus => "HIDDEN+",
            Self::SudHid => "SUD+ & HID+",
            Self::Lift => "LIFT",
            Self::LiftSud => "LIFT & SUD+",
        }
    }
}
