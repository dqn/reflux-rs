use serde::{Deserialize, Serialize};
use strum::{EnumString, FromRepr, IntoStaticStr};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    FromRepr,
    EnumString,
    IntoStaticStr,
)]
#[repr(u8)]
pub enum Difficulty {
    #[strum(serialize = "SPB")]
    SpB = 0,
    #[strum(serialize = "SPN")]
    SpN = 1,
    #[strum(serialize = "SPH")]
    SpH = 2,
    #[strum(serialize = "SPA")]
    SpA = 3,
    #[strum(serialize = "SPL")]
    SpL = 4,
    #[strum(serialize = "DPB")]
    DpB = 5,
    #[strum(serialize = "DPN")]
    DpN = 6,
    #[strum(serialize = "DPH")]
    DpH = 7,
    #[strum(serialize = "DPA")]
    DpA = 8,
    #[strum(serialize = "DPL")]
    DpL = 9,
}

impl Difficulty {
    pub fn from_u8(value: u8) -> Option<Self> {
        Self::from_repr(value)
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
        self.into()
    }

    /// Get the expanded difficulty name (e.g., "NORMAL", "HYPER")
    pub fn expand_name(&self) -> &'static str {
        match self {
            Self::SpB | Self::DpB => "BEGINNER",
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

impl std::fmt::Display for Difficulty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short_name())
    }
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
}
