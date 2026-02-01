//! Status command implementation.

use anyhow::{Result, bail};
use reflux_core::config::find_game_version;
use reflux_core::{
    MemoryReader, OffsetSearcher, ProcessHandle, StatusInfo, builtin_signatures, load_offsets,
};

/// Run the status command
pub fn run(offsets_file: Option<&str>, pid: Option<u32>, json: bool) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Reflux-RS {} - Status Mode", current_version);

    // Open process
    let process = if let Some(pid) = pid {
        println!("Opening process with PID {}...", pid);
        ProcessHandle::open(pid)?
    } else {
        println!("Searching for INFINITAS...");
        ProcessHandle::find_and_open()?
    };

    println!(
        "Found process (PID: {}, Base: 0x{:X}, Size: 0x{:X})",
        process.pid, process.base_address, process.module_size
    );

    let reader = MemoryReader::new(&process);

    // Game version detection
    let game_version = match find_game_version(&reader, process.base_address) {
        Ok(Some(version)) => {
            println!("Game version: {}", version);
            Some(version)
        }
        Ok(None) => {
            println!("Could not detect game version");
            None
        }
        Err(e) => {
            println!("Failed to check game version: {}", e);
            None
        }
    };

    // Load or search for offsets
    let offsets = if let Some(path) = offsets_file {
        match load_offsets(path) {
            Ok(offsets) => {
                println!("Loaded offsets from {}", path);
                offsets
            }
            Err(e) => {
                bail!("Failed to load offsets from {}: {}", path, e);
            }
        }
    } else {
        println!("Searching for offsets...");
        let signatures = builtin_signatures();
        let mut searcher = OffsetSearcher::new(&reader);
        match searcher.search_all_with_signatures(&signatures) {
            Ok(mut offsets) => {
                if let Some(ref version) = game_version {
                    offsets.version = version.clone();
                }
                offsets
            }
            Err(e) => {
                bail!("Failed to detect offsets: {}", e);
            }
        }
    };

    // Collect status
    let status = StatusInfo::collect(
        &reader,
        process.pid,
        process.base_address,
        process.module_size as u64,
        game_version,
        &offsets,
    );

    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!();
        println!("=== Offset Status ===");
        println!(
            "SongList:     0x{:016X}  {}",
            status.offsets.song_list.address,
            if status.offsets.song_list.valid {
                "✓"
            } else {
                "✗"
            }
        );
        println!("              {}", status.offsets.song_list.reason);
        println!(
            "JudgeData:    0x{:016X}  {}",
            status.offsets.judge_data.address,
            if status.offsets.judge_data.valid {
                "✓"
            } else {
                "✗"
            }
        );
        println!("              {}", status.offsets.judge_data.reason);
        println!(
            "PlaySettings: 0x{:016X}  {}",
            status.offsets.play_settings.address,
            if status.offsets.play_settings.valid {
                "✓"
            } else {
                "✗"
            }
        );
        println!("              {}", status.offsets.play_settings.reason);
        println!(
            "PlayData:     0x{:016X}  {}",
            status.offsets.play_data.address,
            if status.offsets.play_data.valid {
                "✓"
            } else {
                "✗"
            }
        );
        println!("              {}", status.offsets.play_data.reason);
        println!(
            "CurrentSong:  0x{:016X}  {}",
            status.offsets.current_song.address,
            if status.offsets.current_song.valid {
                "✓"
            } else {
                "✗"
            }
        );
        println!("              {}", status.offsets.current_song.reason);
        println!(
            "DataMap:      0x{:016X}  {}",
            status.offsets.data_map.address,
            if status.offsets.data_map.valid {
                "✓"
            } else {
                "✗"
            }
        );
        println!("              {}", status.offsets.data_map.reason);
        println!(
            "UnlockData:   0x{:016X}  {}",
            status.offsets.unlock_data.address,
            if status.offsets.unlock_data.valid {
                "✓"
            } else {
                "✗"
            }
        );
        println!("              {}", status.offsets.unlock_data.reason);

        println!();
        println!("=== Song Database ===");
        println!("Songs found: {}", status.song_count);

        if let Some(ref current) = status.current_song {
            println!();
            println!("=== Current Song ===");
            println!("Song ID: {}", current.song_id);
            println!("Difficulty: {}", current.difficulty);
            if let Some(ref title) = current.title {
                println!("Title: {}", title);
            }
        }

        println!();
        println!(
            "Overall validation: {}",
            if status.all_valid { "PASSED" } else { "FAILED" }
        );
    }

    Ok(())
}
