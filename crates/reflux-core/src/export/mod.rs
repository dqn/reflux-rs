//! Export formats for play data and tracking data.
//!
//! This module provides various export formats for play data:
//! - TSV (Tab-Separated Values) for spreadsheet compatibility
//! - JSON for programmatic access
//!
//! # Module Structure
//!
//! - [`format`]: The `ExportFormat` trait definition
//! - [`tsv`]: TSV export implementation
//! - [`json`]: JSON export implementation
//! - [`console`]: Console output with colored display
//! - [`comparison`]: Personal best comparison logic
//! - [`tracker`]: Tracker data export (TSV/JSON)
//!
//! # ExportFormat Trait
//!
//! The `ExportFormat` trait provides a common interface for different export formats:
//!
//! ```ignore
//! use reflux_core::export::{ExportFormat, TsvExporter, JsonExporter};
//!
//! let tsv = TsvExporter;
//! println!("{}", tsv.header());
//! println!("{}", tsv.format_row(&play_data));
//!
//! let json = JsonExporter;
//! println!("{}", json.format_row(&play_data));
//! ```

mod comparison;
mod console;
mod format;
mod json;
mod tracker;
mod tsv;

// Re-export format trait
pub use format::ExportFormat;

// Re-export exporters
pub use json::JsonExporter;
pub use tsv::TsvExporter;

// Re-export TSV functions
pub use tsv::{
    TsvRowData, format_full_tsv_header, format_full_tsv_row, format_tsv_header, format_tsv_row,
};

// Re-export JSON functions
pub use json::{JudgeJson, PlayDataJson, format_json_entry};

// Re-export console functions
pub use console::{format_play_data_console, format_play_summary};

// Re-export comparison types and functions
pub use comparison::{PersonalBestComparison, compare_with_personal_best};

// Re-export tracker functions and types
pub use tracker::{
    ChartDataJson, ExportDataJson, SongDataJson, export_song_list, export_tracker_json,
    export_tracker_tsv, format_tracker_tsv_header, generate_tracker_json, generate_tracker_tsv,
};
