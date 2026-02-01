//! Find offsets command implementation.
//!
//! Interactive mode for discovering memory offsets in new game versions.
//! Requires user interaction (playing a song) to detect play-related offsets
//! through state changes.
//!
//! The output file can be used as input for other commands via `--offsets-file`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reflux_core::config::find_game_version;
use reflux_core::{MemoryReader, OffsetSearcher, OffsetsCollection, ProcessHandle, save_offsets};
use tracing::{debug, info, warn};

use crate::prompter::CliPrompter;
use crate::shutdown::ShutdownSignal;

/// Run the find-offsets interactive mode
pub fn run(output: &str, pid: Option<u32>) -> Result<()> {
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
