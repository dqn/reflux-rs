use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr};
use thiserror::Error;
use tracing::warn;

use crate::play::PlayType;

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

    /// Build settings from raw memory values.
    ///
    /// Invalid enum values are replaced with defaults and logged as warnings.
    /// This can occur when memory contains unexpected values during state transitions.
    #[allow(clippy::too_many_arguments)] // Mapping raw memory layout requires many parameters
    pub fn from_raw_values(
        play_type: PlayType,
        style_val: i32,
        style2_val: i32,
        assist_val: i32,
        range_val: i32,
        flip_val: i32,
        battle_val: i32,
        h_ran_val: i32,
    ) -> Self {
        let style = style_val.try_into().unwrap_or_else(|_| {
            warn!("Invalid style value: {}, using default", style_val);
            Style::default()
        });

        let style2 = if play_type == PlayType::Dp {
            Some(style2_val.try_into().unwrap_or_else(|_| {
                warn!("Invalid style2 value: {}, using default", style2_val);
                Style::default()
            }))
        } else {
            None
        };

        let assist = assist_val.try_into().unwrap_or_else(|_| {
            warn!("Invalid assist value: {}, using default", assist_val);
            AssistType::default()
        });

        let range = range_val.try_into().unwrap_or_else(|_| {
            warn!("Invalid range value: {}, using default", range_val);
            RangeType::default()
        });

        Self {
            style,
            style2,
            assist,
            range,
            flip: flip_val == 1,
            battle: battle_val == 1,
            h_ran: h_ran_val == 1,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, IntoStaticStr, Display,
)]
#[repr(i32)]
pub enum Style {
    #[default]
    #[strum(serialize = "OFF")]
    Off = 0,
    #[strum(serialize = "RANDOM")]
    Random = 1,
    #[strum(serialize = "R-RANDOM")]
    RRandom = 2,
    #[strum(serialize = "S-RANDOM")]
    SRandom = 3,
    #[strum(serialize = "MIRROR")]
    Mirror = 4,
    #[strum(serialize = "SYNCHRONIZE RANDOM")]
    SynchronizeRandom = 5,
    #[strum(serialize = "SYMMETRY RANDOM")]
    SymmetryRandom = 6,
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
        self.into()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, IntoStaticStr, Display,
)]
#[repr(i32)]
pub enum AssistType {
    #[default]
    #[strum(serialize = "OFF")]
    Off = 0,
    #[strum(serialize = "AUTO SCRATCH")]
    AutoScratch = 1,
    #[strum(serialize = "5KEYS")]
    FiveKeys = 2,
    #[strum(serialize = "LEGACY NOTE")]
    LegacyNote = 3,
    #[strum(serialize = "KEY ASSIST")]
    KeyAssist = 4,
    #[strum(serialize = "ANY KEY")]
    AnyKey = 5,
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
        self.into()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, IntoStaticStr, Display,
)]
#[repr(i32)]
pub enum RangeType {
    #[default]
    #[strum(serialize = "OFF")]
    Off = 0,
    #[strum(serialize = "SUDDEN+")]
    SuddenPlus = 1,
    #[strum(serialize = "HIDDEN+")]
    HiddenPlus = 2,
    #[strum(serialize = "SUD+ & HID+")]
    SudHid = 3,
    #[strum(serialize = "LIFT")]
    Lift = 4,
    #[strum(serialize = "LIFT & SUD+")]
    LiftSud = 5,
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
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_style_try_from_valid() {
        assert_eq!(Style::try_from(0).unwrap(), Style::Off);
        assert_eq!(Style::try_from(1).unwrap(), Style::Random);
        assert_eq!(Style::try_from(4).unwrap(), Style::Mirror);
        assert_eq!(Style::try_from(6).unwrap(), Style::SymmetryRandom);
    }

    #[test]
    fn test_style_try_from_invalid() {
        assert!(Style::try_from(7).is_err());
        assert!(Style::try_from(-1).is_err());
        assert!(Style::try_from(100).is_err());
    }

    #[test]
    fn test_assist_type_try_from_valid() {
        assert_eq!(AssistType::try_from(0).unwrap(), AssistType::Off);
        assert_eq!(AssistType::try_from(1).unwrap(), AssistType::AutoScratch);
        assert_eq!(AssistType::try_from(5).unwrap(), AssistType::AnyKey);
    }

    #[test]
    fn test_assist_type_try_from_invalid() {
        assert!(AssistType::try_from(6).is_err());
        assert!(AssistType::try_from(-1).is_err());
    }

    #[test]
    fn test_range_type_try_from_valid() {
        assert_eq!(RangeType::try_from(0).unwrap(), RangeType::Off);
        assert_eq!(RangeType::try_from(1).unwrap(), RangeType::SuddenPlus);
        assert_eq!(RangeType::try_from(5).unwrap(), RangeType::LiftSud);
    }

    #[test]
    fn test_range_type_try_from_invalid() {
        assert!(RangeType::try_from(6).is_err());
        assert!(RangeType::try_from(-1).is_err());
    }

    #[test]
    fn test_settings_from_raw_values_p1() {
        let settings = Settings::from_raw_values(PlayType::P1, 1, 0, 0, 1, 0, 0, 0);
        assert_eq!(settings.style, Style::Random);
        assert!(settings.style2.is_none());
        assert_eq!(settings.range, RangeType::SuddenPlus);
        assert!(!settings.flip);
        assert!(!settings.battle);
        assert!(!settings.h_ran);
    }

    #[test]
    fn test_settings_from_raw_values_dp() {
        let settings = Settings::from_raw_values(PlayType::Dp, 4, 1, 0, 0, 1, 1, 1);
        assert_eq!(settings.style, Style::Mirror);
        assert_eq!(settings.style2, Some(Style::Random));
        assert!(settings.flip);
        assert!(settings.battle);
        assert!(settings.h_ran);
    }

    #[test]
    fn test_settings_from_raw_values_invalid_defaults() {
        // Invalid values should default to Off
        let settings = Settings::from_raw_values(PlayType::P1, 100, 0, 100, 100, 0, 0, 0);
        assert_eq!(settings.style, Style::Off);
        assert_eq!(settings.assist, AssistType::Off);
        assert_eq!(settings.range, RangeType::Off);
    }

    #[test]
    fn test_invalid_enum_value_error_display() {
        let err = InvalidEnumValueError::new("TestEnum", 42);
        assert_eq!(format!("{}", err), "Invalid TestEnum value: 42");
    }
}
