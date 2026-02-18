//! # infst
//!
//! Core library for the INFST score tracker.
//!
//! This crate provides:
//! - Game data structures (PlayData, Judge, Settings, etc.)
//! - Windows process memory reading
//! - Offset detection via signature scanning
//! - Score tracking and session management
//!
//! ## Feature Flags
//!
//! - `debug-tools`: Enables debug utilities for memory analysis and offset verification.
//!   This feature is intended for CLI tools and development, not production use.

pub mod chart;
pub mod config;
#[cfg(feature = "debug-tools")]
pub mod debug;
pub mod error;
pub mod export;
pub mod infst;
pub mod offset;
pub mod play;
pub mod prelude;
pub mod process;
pub mod retry;
pub mod score;
pub mod session;

// Re-export from chart module
pub use chart::{
    Chart, ChartInfo, Difficulty, SongInfo, UnlockData, fetch_song_database,
    fetch_song_database_bulk, get_unlock_state_for_difficulty, get_unlock_states,
};

// Re-export from config module
pub use config::{check_version_match, extract_date_code, find_game_version};

// Re-export from error module
pub use error::{Error, Result};

// Re-export from process module
pub use process::{
    ByteBuffer, MemoryReader, ProcessHandle, ProcessInfo, ProcessProvider, ReadMemory,
    decode_shift_jis, decode_shift_jis_to_string,
};

// Re-export from offset module
pub use offset::{
    CodeSignature, InteractiveSearchResult, JudgeInput, OffsetCache, OffsetDump, OffsetSearcher,
    OffsetSearcherBuilder, OffsetSignatureEntry, OffsetSignatureSet, OffsetsCollection,
    SearchPrompter, SearchResult, builtin_signatures, load_offsets, load_signatures, save_offsets,
    save_offsets_to_cache, save_signatures, try_load_cached_offsets,
};

// Re-export from play module
pub use play::{
    AssistType, GameState, GameStateDetector, PlayData, PlayType, RangeType, Settings, Style,
    UnlockType, calculate_dj_points, calculate_dj_points_from_score,
};

// Re-export from infst module
pub use infst::{ApiConfig, GameData, Infst, InfstConfig, InfstConfigBuilder};

// Re-export from retry module
pub use retry::{ExponentialBackoff, FixedDelay, NoRetry, RetryStrategy};

// Re-export from score module
pub use score::{Grade, Judge, Lamp, ScoreData, ScoreMap};

// Re-export from export module
pub use export::{
    ExportFormat, JsonExporter, TsvExporter, TsvRowData, export_song_list, export_tracker_json,
    export_tracker_tsv, format_tracker_tsv_header, generate_tracker_json, generate_tracker_tsv,
};

// Re-export from session module
pub use session::SessionManager;

// Debug utilities (requires debug-tools feature)
#[cfg(feature = "debug-tools")]
pub use debug::{
    DumpInfo, MemoryDump, OffsetStatus, OffsetValidation, ScanResult, ScannedSong, StatusInfo,
};
