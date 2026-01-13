use anyhow::Result;
use clap::Parser;
use reflux_core::{
    fetch_song_database, load_offsets, Config, MemoryReader, ProcessHandle, Reflux, ScoreMap,
};
use std::path::PathBuf;
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

    let args = Args::parse();

    info!("Reflux starting...");

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

    // Main loop: wait for process
    loop {
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

                // Load score map from game memory
                info!("Loading score map...");
                let _score_map =
                    match ScoreMap::load_from_memory(&reader, reflux.offsets().data_map, &song_db) {
                        Ok(map) => {
                            info!("Loaded {} score entries", map.len());
                            map
                        }
                        Err(e) => {
                            warn!("Failed to load score map: {}", e);
                            ScoreMap::new()
                        }
                    };

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

        thread::sleep(Duration::from_secs(5));
    }
}
