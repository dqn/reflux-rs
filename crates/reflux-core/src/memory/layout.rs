//! Memory layout constants for INFINITAS data structures
//!
//! This module centralizes all memory layout constants used for reading game data.
//! Constants are organized by structure type and derived from the original C# Reflux
//! implementation.
//!
//! # Memory Structure Overview
//!
//! The game uses several key memory structures:
//! - **JudgeData**: Stores real-time judgment counts during play
//! - **PlayData**: Contains play result information (song ID, difficulty, lamp)
//! - **PlaySettings**: Player options (style, gauge, assist, etc.)
//!
//! # Relative Offset Relationships
//!
//! The offset searcher uses these approximate relationships:
//! - JudgeData - PlaySettings ≈ 0x2ACE00
//! - JudgeData + SongList ≈ 0x94E000
//! - PlaySettings + PlayData ≈ 0x2B0
//! - JudgeData + CurrentSong ≈ 0x1E4

/// Memory layout constants for JudgeData structure
///
/// # Structure Layout
///
/// ```text
/// Offset   Field              Size    Description
/// ──────────────────────────────────────────────────────
/// 0x00     P1 PGreat          4       Player 1 Perfect Great count
/// 0x04     P1 Great           4       Player 1 Great count
/// 0x08     P1 Good            4       Player 1 Good count
/// 0x0C     P1 Bad             4       Player 1 Bad count
/// 0x10     P1 Poor            4       Player 1 Poor count
/// 0x14     P2 PGreat          4       Player 2 Perfect Great count
/// 0x18     P2 Great           4       Player 2 Great count
/// 0x1C     P2 Good            4       Player 2 Good count
/// 0x20     P2 Bad             4       Player 2 Bad count
/// 0x24     P2 Poor            4       Player 2 Poor count
/// 0x28     P1 ComboBreak      4       Player 1 combo break count
/// 0x2C     P2 ComboBreak      4       Player 2 combo break count
/// 0x30     P1 Fast            4       Player 1 fast timing count
/// 0x34     P2 Fast            4       Player 2 fast timing count
/// 0x38     P1 Slow            4       Player 1 slow timing count
/// 0x3C     P2 Slow            4       Player 2 slow timing count
/// 0x40     P1 MeasureEnd      4       Player 1 premature end marker
/// 0x44     P2 MeasureEnd      4       Player 2 premature end marker
/// ...      (reserved)         ...
/// 0xD8     StateMarker1       4       Non-zero during play
/// 0xDC     StateMarker2       4       Non-zero during play
/// ```
pub mod judge {
    /// Word size (4 bytes / 32-bit integer)
    pub const WORD: u64 = 4;

    // Player 1 judge data (offsets 0-4)
    pub const P1_PGREAT: u64 = 0;
    pub const P1_GREAT: u64 = WORD;
    pub const P1_GOOD: u64 = WORD * 2;
    pub const P1_BAD: u64 = WORD * 3;
    pub const P1_POOR: u64 = WORD * 4;

    // Player 2 judge data (offsets 5-9)
    pub const P2_PGREAT: u64 = WORD * 5;
    pub const P2_GREAT: u64 = WORD * 6;
    pub const P2_GOOD: u64 = WORD * 7;
    pub const P2_BAD: u64 = WORD * 8;
    pub const P2_POOR: u64 = WORD * 9;

    // Combo break data (offsets 10-11)
    pub const P1_COMBO_BREAK: u64 = WORD * 10;
    pub const P2_COMBO_BREAK: u64 = WORD * 11;

    // Fast/Slow data (offsets 12-15)
    pub const P1_FAST: u64 = WORD * 12;
    pub const P2_FAST: u64 = WORD * 13;
    pub const P1_SLOW: u64 = WORD * 14;
    pub const P2_SLOW: u64 = WORD * 15;

    // Measure end markers (offsets 16-17)
    pub const P1_MEASURE_END: u64 = WORD * 16;
    pub const P2_MEASURE_END: u64 = WORD * 17;

    // Game state detection markers (offsets 54-55)
    pub const STATE_MARKER_1: u64 = WORD * 54;
    pub const STATE_MARKER_2: u64 = WORD * 55;

    /// Size of initial zero region in song select state (18 i32 values = 72 bytes)
    /// P1 (5) + P2 (5) + CB (2) + Fast/Slow (4) + MeasureEnd (2) = 18
    pub const INITIAL_ZERO_SIZE: usize = 72;

    // Validation constants for during-play detection
    /// Maximum total notes in a song (realistic upper bound)
    pub const MAX_NOTES: i32 = 3000;
    /// Maximum combo breaks in a song
    pub const MAX_COMBO_BREAK: i32 = 500;
    /// Maximum fast/slow count
    pub const MAX_FAST_SLOW: i32 = 1000;
}

/// Memory layout constants for PlayData structure
pub mod play {
    pub const WORD: u64 = 4;

    pub const SONG_ID: u64 = 0;
    pub const DIFFICULTY: u64 = WORD;
    pub const LAMP: u64 = WORD * 6;
}

/// Memory layout constants for PlaySettings structure
pub mod settings {
    pub const WORD: u64 = 4;

    /// Song select marker position (negative offset from PlaySettings)
    pub const SONG_SELECT_MARKER: u64 = WORD * 6;
}

/// Timing constants for polling and rate limiting
pub mod timing {
    /// Interval between game state checks in the main loop (ms)
    pub const GAME_STATE_POLL_INTERVAL_MS: u64 = 100;

    /// Delay between API requests when syncing scores to avoid server overload (ms)
    pub const SERVER_SYNC_REQUEST_DELAY_MS: u64 = 20;
}
