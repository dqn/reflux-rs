//! Offset searcher for INFINITAS memory
//!
//! This module provides functionality to locate game data structures in memory.
//! It uses a combination of pattern matching and relative offset calculations.
//!
//! ## Architecture
//!
//! The searcher is split into several submodules:
//!
//! - [`core`]: Core `OffsetSearcher` structure and basic operations
//! - [`song_list`]: SongList detection using version string patterns
//! - [`relative_search`]: Relative offset search from anchor points
//! - [`data_map`]: DataMap and UnlockData detection
//! - [`buffer`]: Buffer management and pattern search helpers
//! - [`interactive`]: User-guided offset discovery workflow
//! - [`validation`]: Offset validation functions
//! - [`pattern`]: Pattern search utilities
//! - [`relative`]: Relative offset utilities
//! - [`legacy`]: Legacy signature-based search (feature-gated)
//!
//! ## Search Strategy
//!
//! The primary search strategy uses SongList as an anchor point:
//!
//! 1. **SongList**: Found via "5.1.1." version string pattern
//! 2. **JudgeData**: Relative offset from SongList (~0x94E3C8 below)
//! 3. **PlaySettings**: Relative offset from JudgeData (~0x2ACFA8 below)
//! 4. **PlayData**: Relative offset from PlaySettings (~0x2A0 above)
//! 5. **CurrentSong**: Relative offset from JudgeData (~0x1E4 above)
//! 6. **DataMap/UnlockData**: Pattern search with validation

mod buffer;
mod constants;
mod core;
mod data_map;
mod interactive;
#[cfg(feature = "legacy-signatures")]
pub mod legacy;
pub mod pattern;
pub mod relative;
mod relative_search;
#[cfg(feature = "legacy-signatures")]
pub mod search;
mod song_list;
mod types;
mod utils;
pub mod validation;

// Re-export core types
pub use core::OffsetSearcher;
pub use types::*;
pub use utils::merge_byte_representations;

// Re-export validation functions and trait
pub use validation::{
    OffsetValidation, validate_basic_memory_access, validate_new_version_text_table,
    validate_signature_offsets,
};
