//! Play-related types and data structures.
//!
//! This module contains types for representing play sessions and game state:
//! - `PlayType` - play types (1P, 2P, DP)
//! - `UnlockType` - unlock types (Base, Bits, Sub)
//! - `GameState` - game states (Unknown, SongSelect, Playing, ResultScreen)
//! - `PlayData` - complete play data
//! - `Settings` - play settings
//! - `GameStateDetector` - game state detection

mod enums;
mod play_data;
mod settings;
mod state;

pub use enums::*;
pub use play_data::*;
pub use settings::*;
pub use state::*;
