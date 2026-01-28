mod input;
mod prompter;
mod shutdown;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use prompter::CliPrompter;
#[cfg(test)]
use reflux_core::UnlockType;
use reflux_core::game::find_game_version;
use reflux_core::{
    CustomTypes, EncodingFixes, MemoryReader, OffsetSearcher, OffsetsCollection, ProcessHandle,
    Reflux, ScoreMap, SongInfo, builtin_signatures, fetch_song_database_with_fixes, load_offsets,
    save_offsets,
};
use shutdown::ShutdownSignal;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

/// Minimum number of songs expected in the song database
const MIN_EXPECTED_SONGS: usize = 1000;
/// Song ID used to verify data readiness (READY FOR TAKEOFF)
const READY_SONG_ID: u32 = 80003;
/// Difficulty index to check for note count (SPA)
const READY_DIFF_INDEX: usize = 3;
/// Minimum note count expected for the reference song
const READY_MIN_NOTES: u32 = 10;

/// Validation result for song database
#[derive(Debug, PartialEq, Eq)]
enum ValidationResult {
    Valid,
    TooFewSongs(usize),
    NotecountTooSmall(u32),
    ReferenceSongMissing,
}

/// Validate the song database is fully populated
fn validate_song_database(db: &HashMap<u32, SongInfo>) -> ValidationResult {
    if db.len() < MIN_EXPECTED_SONGS {
        return ValidationResult::TooFewSongs(db.len());
    }

    if let Some(song) = db.get(&READY_SONG_ID) {
        let notes = song.total_notes.get(READY_DIFF_INDEX).copied().unwrap_or(0);
        if notes < READY_MIN_NOTES {
            return ValidationResult::NotecountTooSmall(notes);
        }
    } else {
        return ValidationResult::ReferenceSongMissing;
    }

    ValidationResult::Valid
}

fn load_song_database_with_retry(
    reader: &MemoryReader,
    song_list: u64,
    encoding_fixes: Option<&EncodingFixes>,
    shutdown: &ShutdownSignal,
) -> Result<Option<HashMap<u32, SongInfo>>> {
    const RETRY_DELAY: Duration = Duration::from_secs(5);
    const EXTRA_DELAY: Duration = Duration::from_secs(1);
    const MAX_ATTEMPTS: u32 = 12;

    let mut attempts = 0u32;
    let mut last_error: Option<String> = None;
    loop {
        // Check for shutdown signal
        if shutdown.is_shutdown() {
            return Ok(None);
        }

        if attempts >= MAX_ATTEMPTS {
            bail!(
                "Failed to load song database after {} attempts: {}",
                MAX_ATTEMPTS,
                last_error.unwrap_or_else(|| "unknown error".to_string())
            );
        }
        attempts += 1;

        // Wait for data initialization (interruptible)
        if shutdown.wait(EXTRA_DELAY) {
            return Ok(None);
        }

        match fetch_song_database_with_fixes(reader, song_list, encoding_fixes) {
            Ok(db) => {
                match validate_song_database(&db) {
                    ValidationResult::Valid => return Ok(Some(db)),
                    ValidationResult::TooFewSongs(count) => {
                        last_error = Some(format!("song list too small ({})", count));
                        warn!(
                            "Song list not fully populated ({} songs), retrying in {}s (attempt {}/{})",
                            count,
                            RETRY_DELAY.as_secs(),
                            attempts,
                            MAX_ATTEMPTS
                        );
                    }
                    ValidationResult::NotecountTooSmall(notes) => {
                        last_error = Some(format!(
                            "notecount too small (song {}, notes {})",
                            READY_SONG_ID, notes
                        ));
                        warn!(
                            "Notecount data seems bad (song {}, notes {}), retrying in {}s (attempt {}/{})",
                            READY_SONG_ID,
                            notes,
                            RETRY_DELAY.as_secs(),
                            attempts,
                            MAX_ATTEMPTS
                        );
                    }
                    ValidationResult::ReferenceSongMissing => {
                        warn!(
                            "Song {} not found in song list, accepting current list",
                            READY_SONG_ID
                        );
                        return Ok(Some(db));
                    }
                }
                // Interruptible retry delay
                if shutdown.wait(RETRY_DELAY) {
                    return Ok(None);
                }
            }
            Err(e) => {
                last_error = Some(e.to_string());
                warn!(
                    "Failed to load song database ({}), retrying in {}s (attempt {}/{})",
                    e,
                    RETRY_DELAY.as_secs(),
                    attempts,
                    MAX_ATTEMPTS
                );
                // Interruptible retry delay
                if shutdown.wait(RETRY_DELAY) {
                    return Ok(None);
                }
            }
        }
    }
}

fn search_offsets_with_retry(
    reader: &MemoryReader,
    game_version: Option<&String>,
    shutdown: &ShutdownSignal,
) -> Result<Option<OffsetsCollection>> {
    const RETRY_DELAY: Duration = Duration::from_secs(5);

    let signatures = builtin_signatures();

    loop {
        // Check for shutdown signal
        if shutdown.is_shutdown() {
            return Ok(None);
        }

        // Interruptible retry delay
        if shutdown.wait(RETRY_DELAY) {
            return Ok(None);
        }

        let mut searcher = OffsetSearcher::new(reader);

        match searcher.search_all_with_signatures(&signatures) {
            Ok(mut offsets) => {
                if let Some(version) = game_version {
                    offsets.version = version.clone();
                }

                if searcher.validate_signature_offsets(&offsets) {
                    return Ok(Some(offsets));
                }

                info!(
                    "Offset validation failed, retrying in {}s...",
                    RETRY_DELAY.as_secs()
                );
            }
            Err(e) => {
                info!(
                    "Offset detection failed ({}), retrying in {}s...",
                    e,
                    RETRY_DELAY.as_secs()
                );
            }
        }
    }
}

#[derive(Parser)]
#[command(name = "reflux")]
#[command(about = "INFINITAS score tracker", version)]
struct Args {
    /// Load offsets from file (skip automatic detection)
    #[arg(long, value_name = "FILE")]
    offsets_file: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Search for memory offsets interactively
    FindOffsets {
        /// Output file path
        #[arg(short, long, default_value = "offsets.txt")]
        output: String,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging (RUST_LOG がなければ warn を既定にする)
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("reflux=warn,reflux_core=warn"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    match args.command {
        Some(Command::FindOffsets { output, pid }) => run_find_offsets(&output, pid),
        None => run_tracking_mode(args.offsets_file.as_deref()),
    }
}

/// Run the find-offsets interactive mode
fn run_find_offsets(output: &str, pid: Option<u32>) -> Result<()> {
    // Setup graceful shutdown handler (Ctrl+C only, no keyboard monitor)
    let shutdown = Arc::new(ShutdownSignal::new());
    let shutdown_ctrlc = Arc::clone(&shutdown);
    ctrlc::set_handler(move || {
        info!("Received shutdown signal, stopping...");
        shutdown_ctrlc.trigger();
    })?;

    let current_version = env!("CARGO_PKG_VERSION");
    info!("Reflux-RS {} - Offset Search Mode", current_version);

    // Open process (either by PID or auto-detect)
    let process = if let Some(pid) = pid {
        println!("Opening process with PID {}...", pid);
        ProcessHandle::open(pid)?
    } else {
        println!("Waiting for INFINITAS... (Press Ctrl+C to cancel)");

        // Wait for process
        loop {
            if shutdown.is_shutdown() {
                info!("Cancelled");
                return Ok(());
            }

            match ProcessHandle::find_and_open() {
                Ok(p) => break p,
                Err(_) => {
                    if shutdown.wait(Duration::from_secs(2)) {
                        info!("Cancelled");
                        return Ok(());
                    }
                }
            }
        }
    };

    debug!(
        "Found INFINITAS process (base: {:#x})",
        process.base_address
    );

    let reader = MemoryReader::new(&process);

    // Game version detection
    let game_version = match find_game_version(&reader, process.base_address) {
        Ok(Some(version)) => {
            println!("Detected game version: {}", version);
            version
        }
        Ok(None) => {
            println!("Could not detect game version, using 'unknown'");
            "unknown".to_string()
        }
        Err(e) => {
            warn!("Failed to check game version: {}", e);
            "unknown".to_string()
        }
    };

    // Run interactive search
    let prompter = CliPrompter;
    let mut searcher = OffsetSearcher::new(&reader);
    let old_offsets = OffsetsCollection::default();

    let result = searcher.interactive_search(&prompter, &old_offsets, &game_version)?;

    // Display results
    println!();
    println!("=== Offset Search Results ===");
    println!("Version:      {}", result.offsets.version);
    println!("Play Type:    {}", result.play_type.short_name());
    println!("SongList:     0x{:X}", result.offsets.song_list);
    println!("JudgeData:    0x{:X}", result.offsets.judge_data);
    println!("PlaySettings: 0x{:X}", result.offsets.play_settings);
    println!("PlayData:     0x{:X}", result.offsets.play_data);
    println!("CurrentSong:  0x{:X}", result.offsets.current_song);
    println!("DataMap:      0x{:X}", result.offsets.data_map);
    println!("UnlockData:   0x{:X}", result.offsets.unlock_data);

    // Save to file
    save_offsets(output, &result.offsets)?;
    println!();
    println!("Offsets saved to: {}", output);

    Ok(())
}

/// Run the main tracking mode
fn run_tracking_mode(offsets_file: Option<&str>) -> Result<()> {
    // Setup graceful shutdown handler
    let shutdown = Arc::new(ShutdownSignal::new());
    let shutdown_ctrlc = Arc::clone(&shutdown);
    ctrlc::set_handler(move || {
        info!("Received shutdown signal, stopping...");
        shutdown_ctrlc.trigger();
    })?;

    // Spawn keyboard input monitor (Esc, q, Q to quit)
    let shutdown_keyboard = Arc::clone(&shutdown);
    let _keyboard_handle = input::spawn_keyboard_monitor(shutdown_keyboard);

    // Print version and check for updates
    let current_version = env!("CARGO_PKG_VERSION");
    info!("Reflux-RS {}", current_version);

    // Load offsets from file if specified
    let initial_offsets = if let Some(path) = offsets_file {
        match load_offsets(path) {
            Ok(offsets) => {
                info!("Loaded offsets from {}", path);
                debug!(
                    "  SongList: {:#x}, JudgeData: {:#x}, PlaySettings: {:#x}",
                    offsets.song_list, offsets.judge_data, offsets.play_settings
                );
                offsets
            }
            Err(e) => {
                warn!("Failed to load offsets from {}: {}", path, e);
                OffsetsCollection::default()
            }
        }
    } else {
        OffsetsCollection::default()
    };

    // Create Reflux instance
    let mut reflux = Reflux::new(initial_offsets);

    // Main loop: wait for process (exits on Ctrl+C, Esc, or q)
    println!("Waiting for INFINITAS... (Press Esc or q to quit)");
    while !shutdown.is_shutdown() {
        match ProcessHandle::find_and_open() {
            Ok(process) => {
                debug!(
                    "Found INFINITAS process (base: {:#x})",
                    process.base_address
                );

                // Create memory reader
                let reader = MemoryReader::new(&process);

                // Game version detection (best-effort)
                let game_version = match find_game_version(&reader, process.base_address) {
                    Ok(Some(version)) => {
                        debug!("Game version: {}", version);
                        Some(version)
                    }
                    Ok(None) => {
                        warn!("Could not detect game version");
                        None
                    }
                    Err(e) => {
                        warn!("Failed to check game version: {}", e);
                        None
                    }
                };

                // Check if offsets are valid before proceeding
                if !reflux.offsets().is_valid() {
                    info!("Invalid offsets detected. Attempting signature search...");

                    let offsets =
                        search_offsets_with_retry(&reader, game_version.as_ref(), &shutdown)?;
                    let Some(offsets) = offsets else {
                        // Shutdown requested during offset search
                        break;
                    };

                    debug!("Signature-based offset detection successful!");
                    reflux.update_offsets(offsets);
                }

                // Load encoding fixes
                let encoding_fixes = match EncodingFixes::load("encodingfixes.txt") {
                    Ok(ef) => {
                        debug!("Loaded {} encoding fixes", ef.len());
                        Some(ef)
                    }
                    Err(e) => {
                        if e.is_not_found() {
                            debug!("Encoding fixes file not found, using defaults");
                        } else {
                            warn!("Failed to load encoding fixes: {}", e);
                        }
                        None
                    }
                };

                // Load song database from game memory
                debug!("Loading song database...");
                let song_db = load_song_database_with_retry(
                    &reader,
                    reflux.offsets().song_list,
                    encoding_fixes.as_ref(),
                    &shutdown,
                )?;
                let Some(song_db) = song_db else {
                    // Shutdown requested during song database loading
                    break;
                };
                debug!("Loaded {} songs", song_db.len());
                reflux.set_song_db(song_db.clone());

                // Load score map from game memory
                debug!("Loading score map...");
                let score_map = match ScoreMap::load_from_memory(
                    &reader,
                    reflux.offsets().data_map,
                    &song_db,
                ) {
                    Ok(map) => {
                        debug!("Loaded {} score entries", map.len());
                        map
                    }
                    Err(e) => {
                        warn!("Failed to load score map: {}", e);
                        ScoreMap::new()
                    }
                };
                reflux.set_score_map(score_map);

                // Load custom types
                match CustomTypes::load("customtypes.txt") {
                    Ok(ct) => {
                        let mut types = std::collections::HashMap::new();
                        let mut parse_failures = 0usize;
                        for (k, v) in ct.iter() {
                            match k.parse::<u32>() {
                                Ok(id) => {
                                    types.insert(id, v.clone());
                                }
                                Err(_) => {
                                    if parse_failures == 0 {
                                        warn!(
                                            "Failed to parse custom type ID '{}' (further errors suppressed)",
                                            k
                                        );
                                    }
                                    parse_failures += 1;
                                }
                            }
                        }
                        if parse_failures > 1 {
                            warn!("{} custom type IDs failed to parse", parse_failures);
                        }
                        debug!("Loaded {} custom types", types.len());
                        reflux.set_custom_types(types);
                    }
                    Err(e) => {
                        if e.is_not_found() {
                            debug!("Custom types file not found, using defaults");
                        } else {
                            warn!("Failed to load custom types: {}", e);
                        }
                    }
                }

                // Load unlock state from memory
                if let Err(e) = reflux.load_unlock_state(&reader) {
                    warn!("Failed to load unlock state: {}", e);
                }

                println!("Ready to track. Waiting for plays...");

                // Run tracker loop
                if let Err(e) = reflux.run(&process, shutdown.as_atomic()) {
                    error!("Tracker error: {}", e);
                }

                // Export tracker.tsv on disconnect
                if let Err(e) = reflux.export_tracker_tsv("tracker.tsv") {
                    error!("Failed to export tracker.tsv: {}", e);
                }

                debug!("Process disconnected, waiting for reconnect...");
            }
            Err(_) => {
                // Process not found, wait and retry
            }
        }

        // Interruptible wait before retry
        if shutdown.wait(Duration::from_secs(5)) {
            break;
        }
    }

    info!("Shutdown complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_song(total_notes: [u32; 10]) -> SongInfo {
        SongInfo {
            id: 0,
            title: "Test".into(),
            title_english: "Test".into(),
            artist: "".into(),
            genre: "".into(),
            bpm: "".into(),
            folder: 0,
            levels: [0; 10],
            total_notes,
            unlock_type: UnlockType::default(),
        }
    }

    #[test]
    fn test_validate_song_database_empty() {
        let db = HashMap::new();
        assert_eq!(
            validate_song_database(&db),
            ValidationResult::TooFewSongs(0)
        );
    }

    #[test]
    fn test_validate_song_database_too_small() {
        let mut db = HashMap::new();
        for i in 0..500 {
            db.insert(i, create_test_song([100; 10]));
        }
        assert_eq!(
            validate_song_database(&db),
            ValidationResult::TooFewSongs(500)
        );
    }

    #[test]
    fn test_validate_song_database_missing_reference_song() {
        let mut db = HashMap::new();
        for i in 0..1100 {
            db.insert(i, create_test_song([100; 10]));
        }
        assert_eq!(
            validate_song_database(&db),
            ValidationResult::ReferenceSongMissing
        );
    }

    #[test]
    fn test_validate_song_database_notecount_too_small() {
        let mut db = HashMap::new();
        for i in 0..1100 {
            db.insert(i, create_test_song([100; 10]));
        }
        // Add reference song with insufficient notes
        db.insert(
            READY_SONG_ID,
            create_test_song([0, 0, 0, 5, 0, 0, 0, 0, 0, 0]),
        );
        assert_eq!(
            validate_song_database(&db),
            ValidationResult::NotecountTooSmall(5)
        );
    }

    #[test]
    fn test_validate_song_database_valid() {
        let mut db = HashMap::new();
        for i in 0..1100 {
            db.insert(i, create_test_song([100; 10]));
        }
        // Add reference song with sufficient notes
        db.insert(
            READY_SONG_ID,
            create_test_song([100, 200, 300, 500, 600, 0, 0, 0, 0, 0]),
        );
        assert_eq!(validate_song_database(&db), ValidationResult::Valid);
    }
}
