//! Debug utilities for analyzing INFINITAS memory structures
//!
//! This module provides tools for:
//! - Checking game and offset status (`StatusInfo`)
//! - Dumping memory structures (`DumpInfo`)
//! - Scanning for song data (`ScanResult`)

mod status;
mod dump;
mod scan;

pub use status::{StatusInfo, OffsetStatus, OffsetValidation};
pub use dump::{DumpInfo, MemoryDump};
pub use scan::{ScanResult, ScannedSong};
