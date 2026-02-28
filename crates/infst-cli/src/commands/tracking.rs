//! Main tracking mode command.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use infst::config::find_game_version;
use infst::input::window;
use infst::{
    ApiConfig, Infst, InfstConfig, MemoryReader, OffsetSearcher, OffsetsCollection, ProcessHandle,
    ScoreMap, SongInfo, load_offsets, save_offsets_to_cache, try_load_cached_offsets,
};
use tracing::{debug, error, info, warn};

use crate::input;
use crate::retry::{load_song_database_with_retry, search_offsets_with_retry};
use crate::shutdown::ShutdownSignal;

/// Run the main tracking mode, launched via URI scheme handler.
///
/// Extracts the token from the URI, launches the game, then enters
/// the normal tracking loop which will pick up the newly started process.
pub fn run_with_uri(uri: &str, api_endpoint: Option<&str>, api_token: Option<&str>) -> Result<()> {
    println!("infst v{}", env!("CARGO_PKG_VERSION"));
    println!("Launching game from URI...");

    let token = infst::launcher::extract_token_from_uri(uri)?;
    let pid = infst::launcher::launch_game(&token)?;
    println!("Game launched (PID: {})", pid);

    run(None, api_endpoint, api_token)
}

/// Run the main tracking mode
pub fn run(
    offsets_file: Option<&str>,
    api_endpoint: Option<&str>,
    api_token: Option<&str>,
) -> Result<()> {
    let shutdown = setup_shutdown_handler();
    let (initial_offsets, offsets_from_file) = load_initial_offsets(offsets_file);

    let config = build_config(api_endpoint, api_token);
    let mut infst = Infst::with_config(initial_offsets, config);

    println!("Waiting for INFINITAS... (Press Esc or q to quit)");

    // Open the game login page if the game is not already running
    if ProcessHandle::find_and_open().is_err() {
        open_login_page();
    }

    while !shutdown.is_shutdown() {
        if let Some(process) = wait_for_process(&shutdown) {
            apply_borderless_if_possible(&process);
            if let Err(e) = run_tracking_session(&mut infst, &process, &shutdown, offsets_from_file)
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

/// Setup graceful shutdown handler with keyboard input
fn setup_shutdown_handler() -> Arc<ShutdownSignal> {
    let shutdown = Arc::new(ShutdownSignal::new());

    // Keyboard input monitor (Esc, q, Q to quit)
    let shutdown_keyboard = Arc::clone(&shutdown);
    let _keyboard_handle = input::spawn_keyboard_monitor(shutdown_keyboard);

    let current_version = env!("CARGO_PKG_VERSION");
    println!("infst v{}", current_version);

    shutdown
}

/// Build InfstConfig with optional API configuration
///
/// Resolves API credentials from: args > credentials file
fn build_config(api_endpoint: Option<&str>, api_token: Option<&str>) -> InfstConfig {
    let api_config = resolve_api_config(api_endpoint, api_token);
    if api_config.is_some() {
        info!("API integration enabled");
    }
    InfstConfig {
        api_config,
        ..InfstConfig::default()
    }
}

/// Resolve API config from args or credentials file
fn resolve_api_config(api_endpoint: Option<&str>, api_token: Option<&str>) -> Option<ApiConfig> {
    // If both are provided via args, use them directly
    if let (Some(endpoint), Some(token)) = (api_endpoint, api_token) {
        return Some(ApiConfig {
            endpoint: endpoint.to_string(),
            token: token.to_string(),
        });
    }

    // Try loading from credentials file
    let creds = super::login::load_credentials();

    let endpoint = api_endpoint
        .map(|s| s.to_string())
        .or_else(|| creds.as_ref().map(|(e, _)| e.clone()))?;
    let token = api_token
        .map(|s| s.to_string())
        .or_else(|| creds.as_ref().map(|(_, t)| t.clone()))?;

    Some(ApiConfig { endpoint, token })
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
    infst: &Infst,
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

    let needs_search = if !infst.offsets().is_valid() {
        info!("Invalid offsets detected (some offsets are zero)");
        true
    } else if offsets_from_file {
        let searcher = OffsetSearcher::new(reader);
        if searcher.validate_basic_memory_access(infst.offsets()) {
            debug!("File-loaded offsets: basic memory access validated");
            false
        } else {
            info!("File-loaded offsets: memory access failed. Attempting signature search...");
            true
        }
    } else {
        let searcher = OffsetSearcher::new(reader);
        if !searcher.validate_signature_offsets(infst.offsets()) {
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

/// Load song database using various strategies
fn load_song_database(
    reader: &MemoryReader,
    song_list: u64,
    shutdown: &ShutdownSignal,
) -> Result<Option<HashMap<u32, SongInfo>>> {
    let tsv_path = "tracker.tsv";

    if std::path::Path::new(tsv_path).exists() {
        debug!("Building song database from TSV + memory scan...");
        let db = infst::chart::build_song_database_from_tsv_with_memory(
            reader, song_list, tsv_path, 0x100000, // 1MB scan
        );

        if db.is_empty() {
            debug!("TSV+memory approach returned empty, trying legacy...");
            return load_song_database_with_retry(reader, song_list, shutdown);
        }
        return Ok(Some(db));
    }

    // No TSV, use memory-only approach
    debug!("No TSV file found, using memory scan...");
    let song_db = infst::chart::fetch_song_database_from_memory_scan(reader, song_list, 0x100000);

    if song_db.is_empty() {
        debug!("Memory scan found no songs, trying legacy approach...");
        return load_song_database_with_retry(reader, song_list, shutdown);
    }

    info!("Loaded {} songs from memory scan", song_db.len());
    Ok(Some(song_db))
}

/// Run a single tracking session with a connected process
fn run_tracking_session(
    infst: &mut Infst,
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
        infst,
        &reader,
        game_version.as_ref(),
        offsets_from_file,
        shutdown,
    )? {
        infst.update_offsets(offsets);
    } else if shutdown.is_shutdown() {
        return Ok(());
    }

    // Load game resources
    let song_db = match load_song_database(&reader, infst.offsets().song_list, shutdown)? {
        Some(db) => db,
        None => return Ok(()), // Shutdown requested
    };

    debug!("Loaded {} songs", song_db.len());
    infst.set_song_db(song_db.clone());

    // Load score map
    let score_map = load_score_map(&reader, infst.offsets().data_map, &song_db);
    infst.set_score_map(score_map);

    // Load unlock state
    if let Err(e) = infst.load_unlock_state(&reader) {
        warn!("Failed to load unlock state: {}", e);
    }

    println!("Ready to track. Waiting for plays...");

    // Run tracker loop
    if let Err(e) = infst.run(process, shutdown.as_atomic()) {
        error!("Tracker error: {}", e);
    }

    // Export tracker.tsv on disconnect
    if let Err(e) = infst.export_tracker_tsv("tracker.tsv") {
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

const WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(500);
const WINDOW_POLL_TIMEOUT: Duration = Duration::from_secs(60);

/// Apply borderless window mode (best-effort, failures are logged and ignored).
fn apply_borderless_if_possible(process: &ProcessHandle) {
    match try_apply_borderless(process) {
        Ok(()) => println!("Borderless window mode applied"),
        Err(e) => warn!("Could not apply borderless mode: {}", e),
    }
}

#[cfg(target_os = "windows")]
fn try_apply_borderless(process: &ProcessHandle) -> anyhow::Result<()> {
    let start = Instant::now();

    let hwnd = loop {
        if start.elapsed() > WINDOW_POLL_TIMEOUT {
            anyhow::bail!("Timed out waiting for game window");
        }

        if !process.is_alive() {
            anyhow::bail!("Game process exited before a window appeared");
        }

        if let Ok(hwnd) = window::find_window_by_pid(process.pid) {
            break hwnd;
        }

        std::thread::sleep(WINDOW_POLL_INTERVAL);
    };

    window::apply_borderless(hwnd)
}

#[cfg(not(target_os = "windows"))]
fn try_apply_borderless(_process: &ProcessHandle) -> anyhow::Result<()> {
    debug!("Borderless window mode is only supported on Windows");
    Ok(())
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

const LOGIN_URL: &str = "https://p.eagate.573.jp/game/infinitas/2/api/login/login.html";

/// Open the INFINITAS login page in the default browser (best-effort).
fn open_login_page() {
    match open::that(LOGIN_URL) {
        Ok(()) => println!("Opened login page in browser"),
        Err(e) => warn!("Could not open browser: {}", e),
    }
}
