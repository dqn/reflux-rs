use anyhow::Result;
use clap::Parser;
use reflux_core::{
    Config, CustomTypes, MemoryReader, ProcessHandle, Reflux, RefluxApi, ScoreMap,
    export_song_list, fetch_song_database, load_offsets,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("reflux=info".parse()?)
                .add_directive("reflux_core=info".parse()?),
        )
        .init();

    // Setup graceful shutdown handler
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    ctrlc::set_handler(move || {
        info!("Received shutdown signal, stopping...");
        r.store(false, Ordering::SeqCst);
    })?;

    let args = Args::parse();

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
            warn!("Failed to load config: {}, using defaults", e);
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
            warn!("Failed to load offsets: {}, using defaults", e);
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
    while running.load(Ordering::SeqCst) {
        info!("Waiting for INFINITAS process...");

        match ProcessHandle::find_and_open() {
            Ok(process) => {
                info!(
                    "Found INFINITAS process (base: {:#x})",
                    process.base_address
                );

                // Create memory reader
                let reader = MemoryReader::new(&process);

                // Check game version
                match reflux.check_game_version(&reader, process.base_address) {
                    Ok((Some(version), matches)) => {
                        info!("Game version: {}", version);
                        if !matches {
                            warn!("Offsets version mismatch, attempting update...");
                            if let Err(e) = reflux.update_support_files(&version, ".").await {
                                warn!("Failed to update support files: {}", e);
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
                        let types: std::collections::HashMap<u32, String> = ct
                            .iter()
                            .filter_map(|(k, v)| k.parse::<u32>().ok().map(|id| (id, v.clone())))
                            .collect();
                        info!("Loaded {} custom types", types.len());
                        reflux.set_custom_types(types);
                    }
                    Err(e) => {
                        warn!("Failed to load custom types: {}", e);
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
