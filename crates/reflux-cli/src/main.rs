use anyhow::Result;
use clap::Parser;
use reflux_core::{
    Config, CustomTypes, MemoryReader, OffsetDump, OffsetSearcher, OffsetsCollection,
    ProcessHandle, Reflux, RefluxApi, ScoreMap, SearchPrompter, export_song_list,
    fetch_song_database, load_from_cache, load_offsets, save_offsets, save_to_cache,
};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

/// CLI prompter for interactive offset search
struct CliPrompter;

impl SearchPrompter for CliPrompter {
    fn prompt_continue(&self, message: &str) {
        println!("{}", message);
        let _ = io::stdout().flush();
        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);
    }

    fn prompt_number(&self, prompt: &str) -> u32 {
        loop {
            print!("{}", prompt);
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok()
                && let Ok(num) = input.trim().parse()
            {
                return num;
            }
            println!("Invalid input, please enter a number");
        }
    }

    fn display_message(&self, message: &str) {
        info!("{}", message);
    }

    fn display_warning(&self, message: &str) {
        warn!("{}", message);
    }
}

#[derive(Parser)]
#[command(name = "reflux")]
#[command(about = "INFINITAS score tracker", version)]
struct Args {
    /// Path to config file
    #[arg(short, long, default_value = "config.ini")]
    config: PathBuf,

    /// Path to offsets file
    #[arg(short, long, default_value = "offsets.txt")]
    offsets: PathBuf,

    /// Path to tracker database file
    #[arg(short, long, default_value = "tracker.db")]
    tracker: PathBuf,

    /// Enable verbose debug output for offset detection
    #[arg(long)]
    debug_offsets: bool,

    /// Dump offset information to JSON file after detection
    #[arg(long)]
    dump_offsets: bool,

    /// Skip automatic detection and use interactive search
    #[arg(long)]
    force_interactive: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging (debug level if --debug-offsets is set)
    let log_level = if args.debug_offsets { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(format!("reflux={}", log_level).parse()?)
                .add_directive(format!("reflux_core={}", log_level).parse()?),
        )
        .init();

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

    // Check for newer version
    match RefluxApi::get_latest_version().await {
        Ok(latest) => {
            let latest_clean = latest.trim_start_matches('v');
            if version_is_newer(latest_clean, current_version) {
                warn!("Newer version {} is available.", latest);
            }
        }
        Err(e) => {
            warn!("Failed to check for updates: {}", e);
        }
    }

    // Load config
    let config = match Config::load(&args.config) {
        Ok(c) => {
            info!("Loaded config from {:?}", args.config);
            c
        }
        Err(e) => {
            if e.is_not_found() {
                info!("Config file not found, using defaults");
            } else {
                warn!("Failed to load config: {}, using defaults", e);
            }
            Config::default()
        }
    };

    // Load offsets
    let offsets = match load_offsets(&args.offsets) {
        Ok(o) => {
            info!("Loaded offsets version: {}", o.version);
            o
        }
        Err(e) => {
            if e.is_not_found() {
                info!("Offsets file not found, using defaults");
            } else {
                warn!("Failed to load offsets: {}, using defaults", e);
            }
            Default::default()
        }
    };

    // Create Reflux instance
    let mut reflux = Reflux::new(config, offsets);

    // Load tracker
    if let Err(e) = reflux.load_tracker(&args.tracker) {
        warn!("Failed to load tracker: {}", e);
    }

    // Main loop: wait for process (exits on Ctrl+C)
    info!("Waiting for INFINITAS process...");
    while running.load(Ordering::SeqCst) {
        match ProcessHandle::find_and_open() {
            Ok(process) => {
                info!(
                    "Found INFINITAS process (base: {:#x})",
                    process.base_address
                );

                // Create memory reader
                let reader = MemoryReader::new(&process);

                // Check game version and update offsets if needed
                let mut offsets_updated = false;
                match reflux.check_game_version(&reader, process.base_address) {
                    Ok((Some(version), matches)) => {
                        info!("Game version: {}", version);
                        if !matches || !reflux.offsets().is_valid() {
                            if !matches {
                                warn!("Offsets version mismatch, attempting update...");
                            } else {
                                warn!("Invalid offsets, attempting update...");
                            }
                            if let Err(e) = reflux.update_support_files(&version, ".").await {
                                warn!("Failed to update support files: {}", e);
                            } else {
                                offsets_updated = true;
                            }
                        }
                    }
                    Ok((None, _)) => {
                        warn!("Could not detect game version");
                    }
                    Err(e) => {
                        warn!("Failed to check game version: {}", e);
                    }
                }

                // Reload offsets if they were updated
                if offsets_updated {
                    match load_offsets(&args.offsets) {
                        Ok(new_offsets) => {
                            if new_offsets.is_valid() {
                                info!("Reloaded valid offsets version: {}", new_offsets.version);
                                reflux = Reflux::new(reflux.config().clone(), new_offsets);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to reload offsets: {}", e);
                        }
                    }
                }

                // Check if offsets are valid before proceeding
                if !reflux.offsets().is_valid() {
                    warn!("Invalid offsets detected. Attempting to find valid offsets...");

                    // Get the game version for the new offsets
                    let version = match reflux.check_game_version(&reader, process.base_address) {
                        Ok((Some(v), _)) => v,
                        _ => String::from("unknown"),
                    };

                    // Step 1: Try loading from version-specific cache
                    let cached_offsets = if args.force_interactive {
                        info!("Skipping cache lookup (--force-interactive)");
                        None
                    } else {
                        match load_from_cache(&version) {
                            Ok(offsets) if offsets.is_valid() => {
                                info!("Loaded valid offsets from cache for version: {}", version);
                                Some(offsets)
                            }
                            Ok(_offsets) => {
                                warn!("Cached offsets are invalid, will search for new ones");
                                None
                            }
                            Err(_) => {
                                info!("No cached offsets found for version: {}", version);
                                None
                            }
                        }
                    };

                    let final_offsets = if let Some(offsets) = cached_offsets {
                        Some(offsets)
                    } else {
                        let mut searcher = OffsetSearcher::new(&reader);

                        // Step 2: Try automatic detection (unless --force-interactive)
                        let search_result = if args.force_interactive {
                            info!("Skipping automatic detection (--force-interactive)");
                            None
                        } else {
                            info!("Attempting automatic offset detection...");
                            match searcher.search_all() {
                                Ok(offsets) => Some(offsets),
                                Err(e) => {
                                    debug!("Automatic offset detection failed: {}", e);
                                    None
                                }
                            }
                        };

                        match search_result {
                            Some(offsets) => {
                                info!("Automatic offset detection successful!");
                                Some(OffsetsCollection {
                                    version: version.clone(),
                                    ..offsets
                                })
                            }
                            None => {
                                // Step 3: Fallback to interactive search
                                if !args.force_interactive {
                                    warn!(
                                        "Automatic detection failed. Falling back to interactive search..."
                                    );
                                }

                                let prompter = CliPrompter;
                                let hint_offsets = OffsetsCollection::default();

                                match searcher.interactive_search(
                                    &prompter,
                                    &hint_offsets,
                                    &version,
                                ) {
                                    Ok(result) => {
                                        info!("Interactive offset search completed successfully!");
                                        Some(result.offsets)
                                    }
                                    Err(e) => {
                                        error!("Interactive offset search also failed: {}", e);
                                        None
                                    }
                                }
                            }
                        }
                    };

                    match final_offsets {
                        Some(offsets) => {
                            // Save to local offsets file
                            if let Err(e) = save_offsets(&args.offsets, &offsets) {
                                error!("Failed to save offsets: {}", e);
                            } else {
                                info!("Saved new offsets to {:?}", args.offsets);
                            }

                            // Save to version-specific cache
                            if let Err(e) = save_to_cache(&offsets) {
                                warn!("Failed to save offsets to cache: {}", e);
                            } else {
                                info!("Saved offsets to cache for version: {}", offsets.version);
                            }

                            // Update reflux with new offsets
                            reflux = Reflux::new(reflux.config().clone(), offsets);
                        }
                        None => {
                            error!(
                                "Cannot proceed with invalid offsets. Please provide valid offsets.txt or run offset search again."
                            );
                            thread::sleep(Duration::from_secs(5));
                            continue;
                        }
                    }
                }

                // Dump offset information if requested
                if args.dump_offsets {
                    let dump =
                        OffsetDump::from_offsets(reflux.offsets(), process.base_address, &reader);
                    match dump.save(Path::new("offset_dump.json")) {
                        Ok(()) => info!("Offset dump saved to offset_dump.json"),
                        Err(e) => warn!("Failed to save offset dump: {}", e),
                    }
                }

                // Load song database from game memory
                info!("Loading song database...");
                let song_db = match fetch_song_database(&reader, reflux.offsets().song_list) {
                    Ok(db) => {
                        info!("Loaded {} songs", db.len());
                        db
                    }
                    Err(e) => {
                        warn!("Failed to load song database: {}", e);
                        std::collections::HashMap::new()
                    }
                };
                reflux.set_song_db(song_db.clone());

                // Output song list for debugging if configured
                if reflux.config().debug.output_db {
                    info!("Outputting song list to songs.tsv...");
                    if let Err(e) = export_song_list("songs.tsv", &song_db) {
                        warn!("Failed to export song list: {}", e);
                    }
                }

                // Load score map from game memory
                info!("Loading score map...");
                let score_map = match ScoreMap::load_from_memory(
                    &reader,
                    reflux.offsets().data_map,
                    &song_db,
                ) {
                    Ok(map) => {
                        info!("Loaded {} score entries", map.len());
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
                        info!("Loaded {} custom types", types.len());
                        reflux.set_custom_types(types);
                    }
                    Err(e) => {
                        if e.is_not_found() {
                            info!("Custom types file not found, using defaults");
                        } else {
                            warn!("Failed to load custom types: {}", e);
                        }
                    }
                }

                // Load unlock database
                if let Err(e) = reflux.load_unlock_db("unlockdb") {
                    warn!("Failed to load unlock db: {}", e);
                }
                if let Err(e) = reflux.load_unlock_state(&reader) {
                    warn!("Failed to load unlock state: {}", e);
                }

                // Sync with server
                if reflux.config().record.save_remote {
                    info!("Syncing with server...");
                    if let Err(e) = reflux.sync_with_server().await {
                        warn!("Server sync failed: {}", e);
                    }
                }

                // Run tracker loop
                if let Err(e) = reflux.run(&process) {
                    error!("Tracker error: {}", e);
                }

                // Save unlock database on disconnect
                if let Err(e) = reflux.save_unlock_db("unlockdb") {
                    error!("Failed to save unlock db: {}", e);
                }

                // Save tracker on disconnect
                if let Err(e) = reflux.save_tracker(&args.tracker) {
                    error!("Failed to save tracker: {}", e);
                }

                info!("Process disconnected, waiting for reconnect...");
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

/// Compare semantic versions to check if latest is newer than current
fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse_version =
        |s: &str| -> Vec<u32> { s.split('.').filter_map(|part| part.parse().ok()).collect() };

    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);

    for i in 0..latest_parts.len().max(current_parts.len()) {
        let latest_num = latest_parts.get(i).copied().unwrap_or(0);
        let current_num = current_parts.get(i).copied().unwrap_or(0);

        if latest_num > current_num {
            return true;
        }
        if latest_num < current_num {
            return false;
        }
    }

    false
}
