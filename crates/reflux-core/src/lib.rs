//! # reflux-core
//!
//! Core library for the Reflux score tracker.
//!
//! This crate provides:
//! - Game data structures (PlayData, Judge, Settings, etc.)
//! - Windows process memory reading
//! - Offset detection via signature scanning
//! - Score tracking and session management

pub mod error;
pub mod game;
pub mod memory;
pub mod offset;
pub mod reflux;
pub mod storage;
pub mod stream;

pub use error::{Error, Result};
pub use game::{
    AssistType, Chart, ChartInfo, CustomTypes, Difficulty, EncodingFixes, GameState,
    GameStateDetector, Grade, Judge, Lamp, PlayData, PlayType, RangeType, Settings, SongInfo,
    Style, UnlockData, UnlockType, calculate_dj_points, calculate_dj_points_from_score,
    fetch_song_database, fetch_song_database_with_fixes, get_unlock_state_for_difficulty,
    get_unlock_states,
};
pub use memory::{MemoryReader, ProcessHandle};
pub use offset::{
    CodeSignature, InteractiveSearchResult, JudgeInput, OffsetDump, OffsetSearcher,
    OffsetSignatureEntry, OffsetSignatureSet, OffsetsCollection, SearchPrompter, SearchResult,
    builtin_signatures, load_offsets, load_signatures, save_offsets, save_signatures,
};
pub use reflux::{GameData, Reflux};
pub use storage::{
    ChartKey, ScoreData, ScoreMap, SessionManager, Tracker, TrackerInfo, TsvRowData, UnlockDb,
    export_song_list, export_tracker_tsv, format_tracker_tsv_header,
};
pub use stream::StreamOutput;
