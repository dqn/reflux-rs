use serde::{Deserialize, Serialize};
use strum::{FromRepr, IntoStaticStr};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, IntoStaticStr,
)]
pub enum PlayType {
    #[default]
    #[strum(serialize = "1P")]
    P1,
    #[strum(serialize = "2P")]
    P2,
    #[strum(serialize = "DP")]
    Dp,
}

impl PlayType {
    pub fn short_name(&self) -> &'static str {
        self.into()
    }
}

impl std::fmt::Display for PlayType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Default,
    FromRepr,
    IntoStaticStr,
)]
#[repr(u8)]
pub enum UnlockType {
    #[default]
    Base = 0,
    Bits = 1,
    Sub = 2,
}

impl UnlockType {
    pub fn from_u8(value: u8) -> Option<Self> {
        Self::from_repr(value)
    }
}

impl std::fmt::Display for UnlockType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, IntoStaticStr)]
pub enum GameState {
    #[default]
    Unknown,
    SongSelect,
    Playing,
    ResultScreen,
}

impl std::fmt::Display for GameState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
