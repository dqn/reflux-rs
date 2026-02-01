//! Chart-related types and data structures.
//!
//! This module contains types for representing charts (songs + difficulties):
//! - `Difficulty` - difficulty levels (SPB, SPN, SPH, SPA, SPL, DPB, DPN, DPH, DPA, DPL)
//! - `Chart`, `ChartInfo` - chart identifiers and metadata
//! - `SongInfo` - song metadata
//! - `UnlockData` - unlock state management

mod chart;
mod difficulty;
mod song;
mod unlock;

pub use chart::*;
pub use difficulty::*;
pub use song::*;
pub use unlock::*;
