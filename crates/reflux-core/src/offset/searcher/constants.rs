//! Search-related constants for offset detection
//!
//! # Search Strategy
//!
//! The offset searcher uses SongList as the primary anchor point, then finds other
//! offsets via relative positions. This approach is reliable because:
//!
//! 1. SongList detection uses song count validation (>= 1000 songs)
//! 2. Relative offsets between structures are stable across game versions
//! 3. JudgeData is found relative to SongList, avoiding false positives
//!
//! # Offset Relationships
//!
//! ```text
//!                        Memory Layout (approximate)
//! ┌─────────────────────────────────────────────────────────┐
//! │                                      SongList ◄──(1)    │
//! │                                          │               │
//! │                                          │ ~0x94E000     │
//! │                                          ▼               │
//! │  PlaySettings  ◄──── 0x2ACEE8 ────► JudgeData ◄──(2)    │
//! │       │                                  │               │
//! │       │ 0x2C0                           │ 0x160         │
//! │       ▼                                  ▼               │
//! │   PlayData ◄──(4)                   CurrentSong ◄──(5)  │
//! │       ▲                                                  │
//! │       │                                                  │
//! │  (3)──┘                                                  │
//! └─────────────────────────────────────────────────────────┘
//!
//! Detection order: (1) SongList → (2) JudgeData → (3) PlaySettings →
//!                  (4) PlayData → (5) CurrentSong
//! ```
//!
//! # Historical Analysis
//!
//! These values are derived from analysis of 9 game versions and remain
//! remarkably stable across updates.

/// Initial buffer size for memory search (2MB)
pub const INITIAL_SEARCH_SIZE: usize = 2 * 1024 * 1024;
/// Maximum half-window size for memory search (total read size is 2x)
pub const MAX_SEARCH_SIZE: usize = 300 * 1024 * 1024;

/// Expected offset from base address to SongList (approximately 49MB)
///
/// Historical analysis shows SongList is typically at base + 0x3100000 to 0x3200000.
/// Using this as a hint allows starting the search near the expected location.
pub const EXPECTED_SONG_LIST_OFFSET: u64 = 0x3180000;

/// Code scan chunk size for signature search (4MB)
pub const CODE_SCAN_CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// Maximum range to scan from base address for signatures (128MB)
pub const CODE_SCAN_LIMIT: usize = 128 * 1024 * 1024;

/// Minimum number of songs expected in INFINITAS (for validation)
pub const MIN_EXPECTED_SONGS: usize = 1000;

/// Minimum valid song ID in IIDX (song IDs start from 1000)
pub const MIN_SONG_ID: i32 = 1000;

/// Maximum valid song ID in IIDX (reasonable upper bound)
pub const MAX_SONG_ID: i32 = 50000;

// ============================================================================
// Relative Offsets (derived from historical analysis of 9 versions)
// ============================================================================

/// Expected offset: judgeData - playSettings ≈ 0x2ACFA8
///
/// Historical values:
/// - Version 1 (2025122400): 0x2ACEE8
/// - Version 2 (2026012800): 0x2ACFA8
///
///   Using Version 2 value as it's the current version.
pub const JUDGE_TO_PLAY_SETTINGS: u64 = 0x2ACFA8;

/// Search range for playSettings (±512 bytes)
///
/// Historical variation between versions is ~192 bytes (0xC0).
/// Using 512 bytes to cover with some margin while avoiding false positives.
pub const PLAY_SETTINGS_SEARCH_RANGE: usize = 0x200;

/// Expected offset: songList - judgeData ≈ 0x94E3C8
///
/// Historical variation: ±0x600 (1.5KB)
pub const JUDGE_TO_SONG_LIST: u64 = 0x94E3C8;

/// Expected offset: playData - playSettings ≈ 0x2A0
///
/// Historical values:
/// - Version 1 (2025122400): 0x2C0 (704 bytes)
/// - Version 2 (2026012800): 0x2A0 (672 bytes)
///
///   Using Version 2 value as it's the current version.
pub const PLAY_SETTINGS_TO_PLAY_DATA: u64 = 0x2A0;

/// Search range for playData (±256 bytes)
///
/// This is ~16x the measured variation to ensure reliable detection.
pub const PLAY_DATA_SEARCH_RANGE: usize = 0x100;

/// Expected offset: currentSong - judgeData ≈ 0x1E4
///
/// Historical variation: ±0x10 (16 bytes)
/// - bm2dx-1 and bm2dx-2: 0x1E4 (same value)
pub const JUDGE_TO_CURRENT_SONG: u64 = 0x1E4;

/// Search range for currentSong (±256 bytes)
///
/// This is ~16x the measured variation to ensure reliable detection.
pub const CURRENT_SONG_SEARCH_RANGE: usize = 0x100;

// ============================================================================
// DataMap validation
// ============================================================================

/// DataMap hash table minimum size (bytes)
pub const DATA_MAP_MIN_TABLE_BYTES: usize = 0x1000;

/// DataMap hash table maximum size (bytes)
pub const DATA_MAP_MAX_TABLE_BYTES: usize = 256 * 1024 * 1024;

/// DataMap hash table scan size (bytes)
pub const DATA_MAP_SCAN_BYTES: usize = 0x4000;

/// DataMap node validation samples
pub const DATA_MAP_NODE_SAMPLES: usize = 32;

/// Search range for judgeData when searching from SongList (±64KB)
///
/// This is the same range as songList search since it uses the same relative offset.
pub const JUDGE_DATA_SEARCH_RANGE: usize = 0x10000;

// ============================================================================
// DataMap entry filtering
// ============================================================================

/// Sentinel value in INFINITAS data map that should be treated as null.
///
/// This specific value (0x494fdce0) appears in the data map hash table as a
/// special marker and should be filtered out when counting valid entries.
pub const DATA_MAP_SENTINEL: u64 = 0x494fdce0;

// ============================================================================
// Address validation
// ============================================================================

/// Expected ImageBase for INFINITAS executable (64-bit Windows default)
///
/// All valid data addresses should be above this value.
pub const IMAGE_BASE: u64 = 0x140000000;

/// Minimum valid data address (ImageBase + typical code section)
///
/// Data sections are typically above the code sections.
pub const MIN_VALID_DATA_ADDRESS: u64 = IMAGE_BASE + 0x1000000;
