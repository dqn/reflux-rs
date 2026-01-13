use anyhow::Result;
use clap::Parser;
use reflux_core::{load_offsets, Config, ProcessHandle, Reflux};
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

fn main() -> Result<()> {
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

                if let Err(e) = reflux.run(&process) {
                    error!("Tracker error: {}", e);
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
