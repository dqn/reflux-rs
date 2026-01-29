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
    CustomTypes, DumpInfo, EncodingFixes, MemoryReader, OffsetSearcher, OffsetsCollection,
    ProcessHandle, ReadMemory, Reflux, ScanResult, ScoreMap, SongInfo, StatusInfo,
    builtin_signatures, fetch_song_database_with_fixes, load_offsets, save_offsets,
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

                // search_all_with_signatures already validates each offset individually
                // (song count, judge data markers, play settings ranges, etc.)
                // so we only need to check that all offsets are non-zero
                if offsets.is_valid() {
                    return Ok(Some(offsets));
                }

                info!(
                    "Offset detection incomplete, retrying in {}s...",
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
    /// Analyze memory structure (debug mode)
    Analyze {
        /// Address to analyze (hex, e.g., 0x14314A50C)
        #[arg(long)]
        address: Option<String>,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
    },
    /// Show game and offset status
    Status {
        /// Load offsets from file
        #[arg(long, value_name = "FILE")]
        offsets_file: Option<String>,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Dump memory structures
    Dump {
        /// Load offsets from file
        #[arg(long, value_name = "FILE")]
        offsets_file: Option<String>,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
        /// Output file path (JSON)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Scan for song database
    Scan {
        /// Load offsets from file
        #[arg(long, value_name = "FILE")]
        offsets_file: Option<String>,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
        /// Scan range in bytes (default: 1MB)
        #[arg(long, default_value = "1048576")]
        range: usize,
        /// TSV file for matching
        #[arg(long)]
        tsv: Option<String>,
        /// Output file path (JSON)
        #[arg(short, long)]
        output: Option<String>,
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
        Some(Command::Analyze { address, pid }) => run_analyze_mode(address, pid),
        Some(Command::Status {
            offsets_file,
            pid,
            json,
        }) => run_status_mode(offsets_file.as_deref(), pid, json),
        Some(Command::Dump {
            offsets_file,
            pid,
            output,
        }) => run_dump_mode(offsets_file.as_deref(), pid, output.as_deref()),
        Some(Command::Scan {
            offsets_file,
            pid,
            range,
            tsv,
            output,
        }) => run_scan_mode(offsets_file.as_deref(), pid, range, tsv.as_deref(), output.as_deref()),
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

/// Run the memory structure analysis mode
fn run_analyze_mode(address: Option<String>, pid: Option<u32>) -> Result<()> {
    // Note: tracing_subscriber is already initialized in main()
    // Analysis output will use println! for user-facing output

    let current_version = env!("CARGO_PKG_VERSION");
    println!("Reflux-RS {} - Memory Analysis Mode", current_version);

    // Open process
    let process = if let Some(pid) = pid {
        println!("Opening process with PID {}...", pid);
        ProcessHandle::open(pid)?
    } else {
        println!("Searching for INFINITAS...");
        ProcessHandle::find_and_open()?
    };

    println!(
        "Found process (Base: 0x{:X}, Size: 0x{:X})",
        process.base_address,
        process.module_size
    );

    let reader = MemoryReader::new(&process);

    // Parse address or search for it
    let analyze_addr = if let Some(addr_str) = address {
        // Parse hex address
        let addr_str = addr_str.trim_start_matches("0x").trim_start_matches("0X");
        u64::from_str_radix(addr_str, 16)?
    } else {
        // Search for new structure using song_id pattern
        println!("No address specified, searching for song data structures...");
        let mut searcher = OffsetSearcher::new(&reader);

        // Try to find 312-byte structure
        match searcher.search_song_list_comprehensive(process.base_address) {
            Ok(addr) => {
                println!("Found song data at: 0x{:X}", addr);
                addr
            }
            Err(e) => {
                bail!("Failed to find song data: {}", e);
            }
        }
    };

    println!();
    println!("=== Analyzing memory at 0x{:X} ===", analyze_addr);

    let searcher = OffsetSearcher::new(&reader);
    searcher.analyze_new_structure(analyze_addr);

    // Also try to read using old SongInfo structure
    println!();
    println!("=== Attempting old structure read ===");
    match SongInfo::read_from_memory(&reader, analyze_addr) {
        Ok(Some(song)) => {
            println!("  Old structure parsed:");
            println!("    id: {}", song.id);
            println!("    title: {:?}", song.title);
            println!("    artist: {:?}", song.artist);
            println!("    folder: {}", song.folder);
            println!("    levels: {:?}", song.levels);
        }
        Ok(None) => println!("  Old structure: Invalid (first 4 bytes are zero)"),
        Err(e) => println!("  Old structure read failed: {}", e),
    }

    // Count songs with old structure
    println!();
    println!("=== Song count analysis ===");
    let old_count = count_songs_old_structure(&reader, analyze_addr);
    println!("  Old structure (0x3F0): {} songs", old_count);

    // Count songs with new structure
    let new_count = count_songs_new_structure(&reader, analyze_addr);
    println!("  New structure (312 bytes): {} songs", new_count);

    // Comprehensive search for song data in memory
    println!();
    println!("=== Searching for song data patterns in memory ===");
    search_song_patterns(&reader, process.base_address, process.module_size as u64);

    // Search for known song titles
    search_for_title_strings(&reader, process.base_address, process.module_size as u64);

    Ok(())
}

/// Search for various song data patterns in memory
fn search_song_patterns(reader: &MemoryReader, base: u64, module_size: u64) {
    // Search for song_id=1001 followed by folder=43 pattern
    let pattern_1001_43: [u8; 8] = [0xE9, 0x03, 0x00, 0x00, 0x2B, 0x00, 0x00, 0x00];

    // Also search for embedded title patterns with valid song_id
    // Try searching for common song titles

    println!("  Searching for song_id=1001 + folder=43 pattern...");

    // Read large chunks of memory and search
    let search_start = base + 0x1000000; // Start 16MB into the module
    let search_end = base + module_size.min(0x5000000); // Up to 80MB
    let chunk_size: usize = 4 * 1024 * 1024; // 4MB chunks

    let mut found_addresses: Vec<u64> = Vec::new();
    let mut offset = 0u64;

    while search_start + offset < search_end {
        let addr = search_start + offset;
        let read_size = chunk_size.min((search_end - addr) as usize);

        match reader.read_bytes(addr, read_size) {
            Ok(buffer) => {
                // Search for pattern
                for (i, window) in buffer.windows(8).enumerate() {
                    if window == pattern_1001_43 {
                        let found_addr = addr + i as u64;
                        found_addresses.push(found_addr);

                        if found_addresses.len() <= 10 {
                            println!("    Found at 0x{:X}", found_addr);

                            // Try to analyze structure at this location
                            analyze_potential_song_entry(reader, found_addr);
                        }
                    }
                }
            }
            Err(_) => {
                // Skip unreadable regions
            }
        }

        offset += chunk_size as u64;
    }

    println!("  Total matches found: {}", found_addresses.len());

    // Also search for consecutive song IDs to find potential song list
    println!();
    println!("  Searching for consecutive song IDs (1001, 1002, 1003)...");
    search_consecutive_song_ids(reader, base, module_size);
}

fn analyze_potential_song_entry(reader: &MemoryReader, addr: u64) {
    // Read 64 bytes around the address
    if let Ok(buffer) = reader.read_bytes(addr, 64) {
        let song_id = i32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let folder = i32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

        // Check if offset 8 has ASCII data (difficulty levels)
        let has_ascii = buffer[8..18].iter().all(|&b| b >= 0x30 && b <= 0x39 || b == 0);

        if has_ascii {
            let diff_str: String = buffer[8..18]
                .iter()
                .take_while(|&&b| b >= 0x30 && b <= 0x39)
                .map(|&b| b as char)
                .collect();
            println!("      song_id={}, folder={}, difficulty=\"{}\"", song_id, folder, diff_str);
        }

        // Check for next entry at various offsets
        for entry_size in [32u64, 48, 64, 80, 96, 128] {
            let next_addr = addr + entry_size;
            if let Ok(next_buf) = reader.read_bytes(next_addr, 8) {
                let next_id = i32::from_le_bytes([next_buf[0], next_buf[1], next_buf[2], next_buf[3]]);
                let next_folder = i32::from_le_bytes([next_buf[4], next_buf[5], next_buf[6], next_buf[7]]);

                // Check if next entry looks valid (song_id 1001-50000, folder 1-50)
                if next_id >= 1000 && next_id <= 50000 && next_folder >= 1 && next_folder <= 50 {
                    println!("        -> Entry size {} works: next song_id={}", entry_size, next_id);
                }
            }
        }
    }
}

fn search_consecutive_song_ids(reader: &MemoryReader, base: u64, module_size: u64) {
    let search_start = base + 0x1000000;
    let search_end = base + module_size.min(0x5000000);
    let chunk_size: usize = 4 * 1024 * 1024;

    let pattern_1001 = [0xE9u8, 0x03, 0x00, 0x00];
    let pattern_1002 = [0xEAu8, 0x03, 0x00, 0x00];

    let mut offset = 0u64;
    let mut found_pairs: Vec<(u64, u64, u64)> = Vec::new(); // (addr_1001, addr_1002, delta)

    while search_start + offset < search_end {
        let addr = search_start + offset;
        let read_size = chunk_size.min((search_end - addr) as usize);

        match reader.read_bytes(addr, read_size) {
            Ok(buffer) => {
                // Find all 1001 patterns
                let mut addr_1001s: Vec<u64> = Vec::new();
                let mut addr_1002s: Vec<u64> = Vec::new();

                for (i, window) in buffer.windows(4).enumerate() {
                    if window == pattern_1001 {
                        addr_1001s.push(addr + i as u64);
                    } else if window == pattern_1002 {
                        addr_1002s.push(addr + i as u64);
                    }
                }

                // Find pairs
                for &a1001 in &addr_1001s {
                    for &a1002 in &addr_1002s {
                        if a1002 > a1001 {
                            let delta = a1002 - a1001;
                            // Look for reasonable entry sizes
                            if delta >= 32 && delta <= 2048 && delta % 4 == 0 {
                                found_pairs.push((a1001, a1002, delta));
                            }
                        }
                    }
                }
            }
            Err(_) => {}
        }

        offset += chunk_size as u64;
    }

    // Group by delta to find likely entry sizes
    let mut delta_counts: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
    for (addr_1001, _, delta) in &found_pairs {
        delta_counts.entry(*delta).or_default().push(*addr_1001);
    }

    // Sort by count
    let mut sorted: Vec<_> = delta_counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    println!("    Top entry size candidates:");
    for (delta, addresses) in sorted.iter().take(5) {
        println!("      Delta={} bytes: {} occurrences", delta, addresses.len());
        if addresses.len() >= 10 {
            // Try to count songs with this structure size
            let first_addr = *addresses.first().unwrap();
            let count = count_songs_with_size(reader, first_addr, *delta);
            println!("        -> Starting at 0x{:X}: {} consecutive songs", first_addr, count);
        }
    }
}

fn count_songs_with_size(reader: &MemoryReader, start: u64, entry_size: u64) -> usize {
    let mut count = 0;
    let mut addr = start;
    let mut prev_id = 0i32;

    while count < 5000 {
        match reader.read_i32(addr) {
            Ok(id) => {
                if id < 1000 || id > 50000 {
                    break;
                }
                // Allow some gaps/out-of-order but not too much
                if count > 0 && (id < prev_id - 500 || id > prev_id + 500) {
                    break;
                }
                prev_id = id;
                count += 1;
                addr += entry_size;
            }
            Err(_) => break,
        }
    }

    count
}

/// Search for known song titles in memory to find where title strings are stored
fn search_for_title_strings(reader: &MemoryReader, base: u64, module_size: u64) {
    println!();
    println!("=== Searching for song title patterns ===");

    // Common ASCII IIDX song titles to search for
    let search_titles: &[(&str, &[u8])] = &[
        ("5.1.1.", b"5.1.1."),
        ("GAMBOL", b"GAMBOL"),
        ("Sleepless", b"Sleepless"),
        ("SLEEPLESS", b"SLEEPLESS"),
        ("piano ambient", b"piano ambient"),
        ("PIANO AMBIENT", b"PIANO AMBIENT"),
        ("R5", b"R5"),
        ("GRADIUSIC CYBER", b"GRADIUSIC CYBER"),
        ("20,november", b"20,november"),
        ("Tangerine Stream", b"Tangerine Stream"),
    ];

    let search_start = base + 0x1000000;
    let search_end = base + module_size.min(0x5000000);
    let chunk_size: usize = 4 * 1024 * 1024;

    for (title, pattern) in search_titles {
        println!("  Searching for \"{}\" ({} bytes)...", title, pattern.len());

        let mut found: Vec<u64> = Vec::new();
        let mut offset = 0u64;

        while search_start + offset < search_end && found.len() < 20 {
            let addr = search_start + offset;
            let read_size = chunk_size.min((search_end - addr) as usize);

            if let Ok(buffer) = reader.read_bytes(addr, read_size) {
                for (i, window) in buffer.windows(pattern.len()).enumerate() {
                    if window == *pattern {
                        let found_addr = addr + i as u64;
                        found.push(found_addr);

                        if found.len() <= 5 {
                            println!("    Found at 0x{:X}", found_addr);

                            // Read some context around the match
                            if let Ok(context) = reader.read_bytes(found_addr.saturating_sub(64), 192) {
                                // Look for song_id nearby (at known offsets from old structure)
                                // Title is at offset 0 in old structure, song_id is at offset 624
                                // So from title position, song_id would be at +624
                                for check_offset in [0usize, 64, 128, 256, 512, 624, 656, 688] {
                                    if check_offset + 4 <= context.len() {
                                        let potential_id = i32::from_le_bytes([
                                            context[check_offset], context[check_offset + 1],
                                            context[check_offset + 2], context[check_offset + 3],
                                        ]);
                                        if potential_id >= 1000 && potential_id <= 50000 {
                                            println!("      -> Potential song_id={} at relative offset {} (abs: 0x{:X})",
                                                potential_id, check_offset as i64 - 64, found_addr.saturating_sub(64) + check_offset as u64);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            offset += chunk_size as u64;
        }

        println!("    Total matches: {}", found.len());
    }
}

fn count_songs_old_structure(reader: &MemoryReader, start: u64) -> usize {
    let mut count = 0;
    let mut addr = start;
    while count < 5000 {
        match SongInfo::read_from_memory(reader, addr) {
            Ok(Some(song)) if !song.title.is_empty() => {
                count += 1;
            }
            _ => break,
        }
        addr += SongInfo::MEMORY_SIZE as u64;
    }
    count
}

fn count_songs_new_structure(reader: &MemoryReader, start: u64) -> usize {
    const NEW_SIZE: u64 = 312;
    let mut count = 0;
    let mut addr = start;
    while count < 5000 {
        let song_id = match reader.read_i32(addr) {
            Ok(id) => id,
            Err(_) => break,
        };
        if song_id < 1000 || song_id > 50000 {
            break;
        }
        count += 1;
        addr += NEW_SIZE;
    }
    count
}

/// Run the status command
fn run_status_mode(offsets_file: Option<&str>, pid: Option<u32>, json: bool) -> Result<()> {
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
            if status.offsets.song_list.valid { "✓" } else { "✗" }
        );
        println!("              {}", status.offsets.song_list.reason);
        println!(
            "JudgeData:    0x{:016X}  {}",
            status.offsets.judge_data.address,
            if status.offsets.judge_data.valid { "✓" } else { "✗" }
        );
        println!("              {}", status.offsets.judge_data.reason);
        println!(
            "PlaySettings: 0x{:016X}  {}",
            status.offsets.play_settings.address,
            if status.offsets.play_settings.valid { "✓" } else { "✗" }
        );
        println!("              {}", status.offsets.play_settings.reason);
        println!(
            "PlayData:     0x{:016X}  {}",
            status.offsets.play_data.address,
            if status.offsets.play_data.valid { "✓" } else { "✗" }
        );
        println!("              {}", status.offsets.play_data.reason);
        println!(
            "CurrentSong:  0x{:016X}  {}",
            status.offsets.current_song.address,
            if status.offsets.current_song.valid { "✓" } else { "✗" }
        );
        println!("              {}", status.offsets.current_song.reason);
        println!(
            "DataMap:      0x{:016X}  {}",
            status.offsets.data_map.address,
            if status.offsets.data_map.valid { "✓" } else { "✗" }
        );
        println!("              {}", status.offsets.data_map.reason);
        println!(
            "UnlockData:   0x{:016X}  {}",
            status.offsets.unlock_data.address,
            if status.offsets.unlock_data.valid { "✓" } else { "✗" }
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

/// Run the dump command
fn run_dump_mode(offsets_file: Option<&str>, pid: Option<u32>, output: Option<&str>) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Reflux-RS {} - Dump Mode", current_version);

    // Open process
    let process = if let Some(pid) = pid {
        ProcessHandle::open(pid)?
    } else {
        ProcessHandle::find_and_open()?
    };

    println!(
        "Found process (PID: {}, Base: 0x{:X})",
        process.pid, process.base_address
    );

    let reader = MemoryReader::new(&process);

    // Load or search for offsets
    let offsets = if let Some(path) = offsets_file {
        load_offsets(path)?
    } else {
        let signatures = builtin_signatures();
        let mut searcher = OffsetSearcher::new(&reader);
        searcher.search_all_with_signatures(&signatures)?
    };

    // Collect dump
    let dump = DumpInfo::collect(&reader, &offsets);

    if let Some(output_path) = output {
        let json = serde_json::to_string_pretty(&dump)?;
        std::fs::write(output_path, json)?;
        println!("Dump saved to: {}", output_path);
    } else {
        // Print summary to stdout
        println!();
        println!("=== Offsets ===");
        println!("{}", serde_json::to_string_pretty(&dump.offsets)?);

        println!();
        println!("=== Song Entries (first {}) ===", dump.song_entries.len());
        for entry in &dump.song_entries {
            println!(
                "  [{}] 0x{:X}: id={}, folder={}, title={:?}",
                entry.index, entry.address, entry.song_id, entry.folder, entry.title
            );
            if let (Some(meta_id), Some(meta_folder)) = (entry.metadata_song_id, entry.metadata_folder) {
                println!("       metadata: id={}, folder={}", meta_id, meta_folder);
            }
        }

        if let Some(ref song_list_dump) = dump.song_list_dump {
            println!();
            println!("=== SongList Memory Dump (first 256 bytes) ===");
            for line in song_list_dump.hex_dump.iter().take(16) {
                println!("  {}", line);
            }
        }

        println!();
        println!("=== Detected Songs ({} total) ===", dump.detected_songs.len());
        for (i, song) in dump.detected_songs.iter().take(20).enumerate() {
            println!(
                "  [{}] id={}, folder={}, title={:?} ({})",
                i, song.song_id, song.folder, song.title, song.source
            );
        }
        if dump.detected_songs.len() > 20 {
            println!("  ... and {} more", dump.detected_songs.len() - 20);
        }
    }

    Ok(())
}

/// Run the scan command
fn run_scan_mode(
    offsets_file: Option<&str>,
    pid: Option<u32>,
    range: usize,
    tsv_file: Option<&str>,
    output: Option<&str>,
) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Reflux-RS {} - Scan Mode", current_version);

    // Open process
    let process = if let Some(pid) = pid {
        ProcessHandle::open(pid)?
    } else {
        ProcessHandle::find_and_open()?
    };

    println!(
        "Found process (PID: {}, Base: 0x{:X})",
        process.pid, process.base_address
    );

    let reader = MemoryReader::new(&process);

    // Load or search for offsets
    let offsets = if let Some(path) = offsets_file {
        load_offsets(path)?
    } else {
        let signatures = builtin_signatures();
        let mut searcher = OffsetSearcher::new(&reader);
        searcher.search_all_with_signatures(&signatures)?
    };

    // Load TSV if provided
    let tsv_db = if let Some(tsv_path) = tsv_file {
        match reflux_core::game::load_song_database_from_tsv(tsv_path) {
            Ok(db) => {
                println!("Loaded {} songs from TSV", db.len());
                Some(db)
            }
            Err(e) => {
                warn!("Failed to load TSV: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Perform scan
    println!("Scanning {} bytes from 0x{:X}...", range, offsets.song_list);
    let scan_result = ScanResult::scan(&reader, offsets.song_list, range, tsv_db.as_ref());

    if let Some(output_path) = output {
        let json = serde_json::to_string_pretty(&scan_result)?;
        std::fs::write(output_path, json)?;
        println!("Scan results saved to: {}", output_path);
    } else {
        // Print summary to stdout
        println!();
        println!("=== Scan Results ===");
        println!("Scan start: 0x{:X}", scan_result.scan_start);
        println!("Scan range: {} bytes", scan_result.scan_range);
        println!("Songs found: {}", scan_result.songs_found);

        println!();
        println!("=== Detected Songs ===");
        for (i, song) in scan_result.songs.iter().take(30).enumerate() {
            println!(
                "  [{}] id={:5}, folder={:2}, title={:?} ({})",
                i, song.song_id, song.folder, song.title, song.source_type
            );
        }
        if scan_result.songs.len() > 30 {
            println!("  ... and {} more", scan_result.songs.len() - 30);
        }

        if let Some(ref matches) = scan_result.tsv_matches {
            println!();
            println!("=== TSV Matching ===");
            println!(
                "Matched: {} / {}",
                scan_result.matched_count.unwrap_or(0),
                matches.len()
            );

            let unmatched: Vec<_> = matches.iter().filter(|m| !m.matched).collect();
            if !unmatched.is_empty() {
                println!();
                println!("Unmatched songs:");
                for m in unmatched.iter().take(10) {
                    println!("  id={}: {:?}", m.song_id, m.memory_title);
                }
                if unmatched.len() > 10 {
                    println!("  ... and {} more", unmatched.len() - 10);
                }
            }
        }
    }

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
                // First check basic validity (all offsets non-zero)
                // Then validate signature offsets against the actual memory state
                let needs_search = if !reflux.offsets().is_valid() {
                    info!("Invalid offsets detected (some offsets are zero)");
                    true
                } else {
                    let searcher = OffsetSearcher::new(&reader);
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

                // Load song database
                // Strategy:
                // 1. If TSV exists, use it as primary source (complete metadata)
                // 2. Scan memory for song_id mappings
                // 3. Match TSV entries with memory song_ids
                // 4. Fall back to memory-only or legacy approach if needed

                let tsv_path = "tracker.tsv";
                let song_db = if std::path::Path::new(tsv_path).exists() {
                    debug!("Building song database from TSV + memory scan...");
                    let db = reflux_core::game::build_song_database_from_tsv_with_memory(
                        &reader,
                        reflux.offsets().song_list,
                        tsv_path,
                        0x100000, // 1MB scan
                    );
                    if db.is_empty() {
                        debug!("TSV+memory approach returned empty, trying legacy...");
                        let legacy_db = load_song_database_with_retry(
                            &reader,
                            reflux.offsets().song_list,
                            encoding_fixes.as_ref(),
                            &shutdown,
                        )?;
                        let Some(db) = legacy_db else {
                            break;
                        };
                        db
                    } else {
                        db
                    }
                } else {
                    // No TSV, use memory-only approach
                    debug!("No TSV file found, using memory scan...");
                    let song_db = reflux_core::game::fetch_song_database_from_memory_scan(
                        &reader,
                        reflux.offsets().song_list,
                        0x100000,
                    );

                    if song_db.is_empty() {
                        debug!("Memory scan found no songs, trying legacy approach...");
                        let db = load_song_database_with_retry(
                            &reader,
                            reflux.offsets().song_list,
                            encoding_fixes.as_ref(),
                            &shutdown,
                        )?;
                        let Some(db) = db else {
                            break;
                        };
                        db
                    } else {
                        info!("Loaded {} songs from memory scan", song_db.len());
                        song_db
                    }
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
