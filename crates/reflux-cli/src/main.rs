use anyhow::{Result, bail};
use clap::Parser;
#[cfg(test)]
use reflux_core::UnlockType;
use reflux_core::game::find_game_version;
use reflux_core::{
    CustomTypes, EncodingFixes, MemoryReader, OffsetSearcher, OffsetsCollection, ProcessHandle,
    Reflux, ScoreMap, SongInfo, builtin_signatures, fetch_song_database_with_fixes,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
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
) -> Result<HashMap<u32, SongInfo>> {
    const RETRY_DELAY_MS: u64 = 5000;
    const EXTRA_DELAY_MS: u64 = 1000;
    const MAX_ATTEMPTS: u32 = 12;

    let mut attempts = 0u32;
    let mut last_error: Option<String> = None;
    loop {
        if attempts >= MAX_ATTEMPTS {
            bail!(
                "Failed to load song database after {} attempts: {}",
                MAX_ATTEMPTS,
                last_error.unwrap_or_else(|| "unknown error".to_string())
            );
        }
        attempts += 1;

        // Wait for data initialization
        thread::sleep(Duration::from_millis(EXTRA_DELAY_MS));

        match fetch_song_database_with_fixes(reader, song_list, encoding_fixes) {
            Ok(db) => {
                match validate_song_database(&db) {
                    ValidationResult::Valid => return Ok(db),
                    ValidationResult::TooFewSongs(count) => {
                        last_error = Some(format!("song list too small ({})", count));
                        warn!(
                            "Song list not fully populated ({} songs), retrying in {}s (attempt {}/{})",
                            count,
                            RETRY_DELAY_MS / 1000,
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
                            RETRY_DELAY_MS / 1000,
                            attempts,
                            MAX_ATTEMPTS
                        );
                    }
                    ValidationResult::ReferenceSongMissing => {
                        warn!(
                            "Song {} not found in song list, accepting current list",
                            READY_SONG_ID
                        );
                        return Ok(db);
                    }
                }
                thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
            }
            Err(e) => {
                last_error = Some(e.to_string());
                warn!(
                    "Failed to load song database ({}), retrying in {}s (attempt {}/{})",
                    e,
                    RETRY_DELAY_MS / 1000,
                    attempts,
                    MAX_ATTEMPTS
                );
                thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
            }
        }
    }
}

fn search_offsets_with_retry(
    reader: &MemoryReader,
    game_version: Option<&String>,
) -> Result<OffsetsCollection> {
    const RETRY_DELAY_MS: u64 = 5000;

    let signatures = builtin_signatures();

    loop {
        thread::sleep(Duration::from_millis(RETRY_DELAY_MS));

        let mut searcher = OffsetSearcher::new(reader);

        match searcher.search_all_with_signatures(&signatures) {
            Ok(mut offsets) => {
                if let Some(version) = game_version {
                    offsets.version = version.clone();
                }

                if searcher.validate_signature_offsets(&offsets) {
                    return Ok(offsets);
                }

                warn!(
                    "Offset validation failed, retrying in {}s...",
                    RETRY_DELAY_MS / 1000
                );
            }
            Err(e) => {
                warn!(
                    "Offset detection failed ({}), retrying in {}s...",
                    e,
                    RETRY_DELAY_MS / 1000
                );
            }
        }
    }
}

#[derive(Parser)]
#[command(name = "reflux")]
#[command(about = "INFINITAS score tracker", version)]
struct Args {}

fn main() -> Result<()> {
    Args::parse();

    // Initialize logging (RUST_LOG がなければ warn を既定にする)
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("reflux=warn,reflux_core=warn"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    // Setup graceful shutdown handler
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    ctrlc::set_handler(move || {
        info!("Received shutdown signal, stopping...");
        r.store(false, Ordering::SeqCst);
    })?;

    // Print version and check for updates
    let current_version = env!("CARGO_PKG_VERSION");
    info!("Reflux-RS {}", current_version);

    // Create Reflux instance
    let mut reflux = Reflux::new(OffsetsCollection::default());

    // Main loop: wait for process (exits on Ctrl+C)
    debug!("Waiting for INFINITAS process...");
    while running.load(Ordering::SeqCst) {
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
                    warn!("Invalid offsets detected. Attempting signature search...");

                    let offsets = search_offsets_with_retry(&reader, game_version.as_ref())?;

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
                )?;
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

                // Run tracker loop
                if let Err(e) = reflux.run(&process) {
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

        // Check if we should continue or exit
        if !running.load(Ordering::SeqCst) {
            break;
        }

        thread::sleep(Duration::from_secs(5));
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
