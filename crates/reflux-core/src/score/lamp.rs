use serde::{Deserialize, Serialize};
use strum::{FromRepr, IntoStaticStr};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    Default,
    FromRepr,
    IntoStaticStr,
)]
#[repr(u8)]
pub enum Lamp {
    #[default]
    #[strum(serialize = "NO PLAY")]
    NoPlay = 0,
    #[strum(serialize = "FAILED")]
    Failed = 1,
    #[strum(serialize = "ASSIST")]
    AssistClear = 2,
    #[strum(serialize = "EASY")]
    EasyClear = 3,
    #[strum(serialize = "CLEAR")]
    Clear = 4,
    #[strum(serialize = "HARD")]
    HardClear = 5,
    #[strum(serialize = "EX HARD")]
    ExHardClear = 6,
    #[strum(serialize = "FC")]
    FullCombo = 7,
    #[strum(serialize = "PFC")]
    Pfc = 8,
}

impl Lamp {
    pub fn from_u8(value: u8) -> Option<Self> {
        Self::from_repr(value)
    }

    pub fn short_name(&self) -> &'static str {
        self.into()
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

impl std::fmt::Display for Lamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lamp_ordering() {
        assert!(Lamp::Pfc > Lamp::FullCombo);
        assert!(Lamp::FullCombo > Lamp::ExHardClear);
        assert!(Lamp::Failed < Lamp::Clear);
    }
}
