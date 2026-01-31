//! Main tracking mode command.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reflux_core::game::find_game_version;
use reflux_core::{
    CustomTypes, EncodingFixes, MemoryReader, OffsetSearcher, OffsetsCollection, ProcessHandle,
    Reflux, ScoreMap, SongInfo, load_offsets, save_offsets_to_cache, try_load_cached_offsets,
};
use tracing::{debug, error, info, warn};

use crate::input;
use crate::retry::{load_song_database_with_retry, search_offsets_with_retry};
use crate::shutdown::ShutdownSignal;

/// Run the main tracking mode
pub fn run(offsets_file: Option<&str>) -> Result<()> {
    let shutdown = setup_shutdown_handler()?;
    let (initial_offsets, offsets_from_file) = load_initial_offsets(offsets_file);

    let mut reflux = Reflux::new(initial_offsets);

    println!("Waiting for INFINITAS... (Press Esc or q to quit)");

    while !shutdown.is_shutdown() {
        if let Some(process) = wait_for_process(&shutdown) {
            if let Err(e) =
                run_tracking_session(&mut reflux, &process, &shutdown, offsets_from_file)
            {
                error!("Tracking session error: {}", e);
            }
            println!("Waiting for INFINITAS...");
        }

        if shutdown.wait(Duration::from_secs(5)) {
            break;
        }
    }

    println!("Shutdown complete.");
    Ok(())
}

/// Setup graceful shutdown handler with Ctrl+C and keyboard input
fn setup_shutdown_handler() -> Result<Arc<ShutdownSignal>> {
    let shutdown = Arc::new(ShutdownSignal::new());

    // Ctrl+C handler
    let shutdown_ctrlc = Arc::clone(&shutdown);
    ctrlc::set_handler(move || {
        println!("\nShutting down...");
        shutdown_ctrlc.trigger();
    })?;

    // Keyboard input monitor (Esc, q, Q to quit)
    let shutdown_keyboard = Arc::clone(&shutdown);
    let _keyboard_handle = input::spawn_keyboard_monitor(shutdown_keyboard);

    let current_version = env!("CARGO_PKG_VERSION");
    println!("Reflux-RS v{}", current_version);

    Ok(shutdown)
}

/// Load offsets from file if specified
fn load_initial_offsets(offsets_file: Option<&str>) -> (OffsetsCollection, bool) {
    if let Some(path) = offsets_file {
        match load_offsets(path) {
            Ok(offsets) => {
                info!("Loaded offsets from {}", path);
                debug!(
                    "  SongList: {:#x}, JudgeData: {:#x}, PlaySettings: {:#x}",
                    offsets.song_list, offsets.judge_data, offsets.play_settings
                );
                (offsets, true)
            }
            Err(e) => {
                warn!("Failed to load offsets from {}: {}", path, e);
                (OffsetsCollection::default(), false)
            }
        }
    } else {
        (OffsetsCollection::default(), false)
    }
}

/// Wait for the game process to become available
fn wait_for_process(shutdown: &ShutdownSignal) -> Option<ProcessHandle> {
    if shutdown.is_shutdown() {
        return None;
    }

    match ProcessHandle::find_and_open() {
        Ok(process) => {
            println!("Connected to INFINITAS (PID: {})", process.pid);
            debug!("Process base: {:#x}", process.base_address);
            Some(process)
        }
        Err(e) => {
            debug!("Process not found: {}", e);
            None
        }
    }
}

/// Validate or search for offsets
///
/// Uses cached offsets if available and valid, otherwise performs a full search.
fn validate_or_search_offsets(
    reflux: &Reflux,
    reader: &MemoryReader,
    game_version: Option<&String>,
    offsets_from_file: bool,
    shutdown: &ShutdownSignal,
) -> Result<Option<OffsetsCollection>> {
    // Try to use cached offsets first (if not loading from file)
    if !offsets_from_file
        && let Some(version) = game_version
        && let Some(cached_offsets) = try_load_cached_offsets(version)
    {
        // Validate cached offsets still work
        let searcher = OffsetSearcher::new(reader);
        if searcher.validate_basic_memory_access(&cached_offsets) {
            info!("Using cached offsets (validated)");
            return Ok(Some(cached_offsets));
        } else {
            info!("Cached offsets invalid, performing fresh search...");
        }
    }

    let needs_search = if !reflux.offsets().is_valid() {
        info!("Invalid offsets detected (some offsets are zero)");
        true
    } else if offsets_from_file {
        let searcher = OffsetSearcher::new(reader);
        if searcher.validate_basic_memory_access(reflux.offsets()) {
            debug!("File-loaded offsets: basic memory access validated");
            false
        } else {
            info!("File-loaded offsets: memory access failed. Attempting signature search...");
            true
        }
    } else {
        let searcher = OffsetSearcher::new(reader);
        if !searcher.validate_signature_offsets(reflux.offsets()) {
            info!(
                "Offset validation failed (offsets may be stale or incorrect). Attempting signature search..."
            );
            true
        } else {
            debug!("Loaded offsets validated successfully");
            false
        }
    };

    if needs_search {
        let offsets = search_offsets_with_retry(reader, game_version, shutdown)?;
        if let Some(ref found_offsets) = offsets {
            debug!("Signature-based offset detection successful!");
            // Save to cache for next startup
            if let Some(version) = game_version {
                save_offsets_to_cache(version, found_offsets);
            }
        }
        Ok(offsets)
    } else {
        Ok(None)
    }
}

/// Load encoding fixes from file
fn load_encoding_fixes() -> Option<EncodingFixes> {
    match EncodingFixes::load("encodingfixes.txt") {
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
    }
}

/// Load song database using various strategies
fn load_song_database(
    reader: &MemoryReader,
    song_list: u64,
    encoding_fixes: Option<&EncodingFixes>,
    shutdown: &ShutdownSignal,
) -> Result<Option<HashMap<u32, SongInfo>>> {
    let tsv_path = "tracker.tsv";

    if std::path::Path::new(tsv_path).exists() {
        debug!("Building song database from TSV + memory scan...");
        let db = reflux_core::game::build_song_database_from_tsv_with_memory(
            reader, song_list, tsv_path, 0x100000, // 1MB scan
        );

        if db.is_empty() {
            debug!("TSV+memory approach returned empty, trying legacy...");
            return load_song_database_with_retry(reader, song_list, encoding_fixes, shutdown);
        }
        return Ok(Some(db));
    }

    // No TSV, use memory-only approach
    debug!("No TSV file found, using memory scan...");
    let song_db =
        reflux_core::game::fetch_song_database_from_memory_scan(reader, song_list, 0x100000);

    if song_db.is_empty() {
        debug!("Memory scan found no songs, trying legacy approach...");
        return load_song_database_with_retry(reader, song_list, encoding_fixes, shutdown);
    }

    info!("Loaded {} songs from memory scan", song_db.len());
    Ok(Some(song_db))
}

/// Load custom types from file
fn load_custom_types() -> HashMap<u32, String> {
    match CustomTypes::load("customtypes.txt") {
        Ok(ct) => {
            let mut types = HashMap::new();
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
            types
        }
        Err(e) => {
            if e.is_not_found() {
                debug!("Custom types file not found, using defaults");
            } else {
                warn!("Failed to load custom types: {}", e);
            }
            HashMap::new()
        }
    }
}

/// Run a single tracking session with a connected process
fn run_tracking_session(
    reflux: &mut Reflux,
    process: &ProcessHandle,
    shutdown: &ShutdownSignal,
    offsets_from_file: bool,
) -> Result<()> {
    println!("Initializing...");
    let reader = MemoryReader::new(process);

    // Game version detection
    let game_version = detect_game_version(&reader, process.base_address);

    // Validate or search for offsets
    if let Some(offsets) = validate_or_search_offsets(
        reflux,
        &reader,
        game_version.as_ref(),
        offsets_from_file,
        shutdown,
    )? {
        reflux.update_offsets(offsets);
    } else if shutdown.is_shutdown() {
        return Ok(());
    }

    // Load game resources
    let encoding_fixes = load_encoding_fixes();

    let song_db = match load_song_database(
        &reader,
        reflux.offsets().song_list,
        encoding_fixes.as_ref(),
        shutdown,
    )? {
        Some(db) => db,
        None => return Ok(()), // Shutdown requested
    };

    debug!("Loaded {} songs", song_db.len());
    reflux.set_song_db(song_db.clone());

    // Load score map
    let score_map = load_score_map(&reader, reflux.offsets().data_map, &song_db);
    reflux.set_score_map(score_map);

    // Load custom types
    let custom_types = load_custom_types();
    reflux.set_custom_types(custom_types);

    // Load unlock state
    if let Err(e) = reflux.load_unlock_state(&reader) {
        warn!("Failed to load unlock state: {}", e);
    }

    println!("Ready to track. Waiting for plays...");

    // Run tracker loop
    if let Err(e) = reflux.run(process, shutdown.as_atomic()) {
        error!("Tracker error: {}", e);
    }

    // Export tracker.tsv on disconnect
    if let Err(e) = reflux.export_tracker_tsv("tracker.tsv") {
        error!("Failed to export tracker.tsv: {}", e);
    }

    Ok(())
}

/// Detect game version (best-effort)
fn detect_game_version(reader: &MemoryReader, base_address: u64) -> Option<String> {
    match find_game_version(reader, base_address) {
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
    }
}

/// Load score map from game memory
fn load_score_map(
    reader: &MemoryReader,
    data_map: u64,
    song_db: &HashMap<u32, SongInfo>,
) -> ScoreMap {
    debug!("Loading score map...");
    match ScoreMap::load_from_memory(reader, data_map, song_db) {
        Ok(map) => {
            debug!("Loaded {} score entries", map.len());
            map
        }
        Err(e) => {
            warn!("Failed to load score map: {}", e);
            ScoreMap::new()
        }
    }
}
