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
    builtin_signatures, fetch_song_database_with_fixes, generate_tracker_json,
    generate_tracker_tsv, get_unlock_states, load_offsets, save_offsets,
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
        /// Entry size in bytes (default: 1200)
        #[arg(long)]
        entry_size: Option<usize>,
    },
    /// Explore memory structure at a specific address
    Explore {
        /// Base address to explore (hex, e.g., 0x1431865A0)
        #[arg(long)]
        address: String,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
    },
    /// Dump raw bytes from memory (hexdump)
    Hexdump {
        /// Start address (hex, e.g., 0x1431B08A0)
        #[arg(long)]
        address: String,
        /// Number of bytes to dump (default: 256)
        #[arg(long, default_value = "256")]
        size: usize,
        /// Include ASCII representation
        #[arg(long)]
        ascii: bool,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
    },
    /// Search for values in memory
    Search {
        /// Search for a string (Shift-JIS encoded)
        #[arg(long)]
        string: Option<String>,
        /// Search for a 32-bit integer
        #[arg(long)]
        i32: Option<i32>,
        /// Search for a 16-bit integer
        #[arg(long)]
        i16: Option<i16>,
        /// Search for a byte pattern (hex, e.g., "00 04 07 0A", use ?? for wildcard)
        #[arg(long)]
        pattern: Option<String>,
        /// Maximum number of results (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
    },
    /// Calculate offset between two addresses
    Offset {
        /// Start address (hex)
        #[arg(long)]
        from: String,
        /// End address (hex)
        #[arg(long)]
        to: String,
    },
    /// Validate memory structures
    Validate {
        #[command(subcommand)]
        target: ValidateTarget,
    },
    /// Export all play data (scores, lamps, miss counts)
    Export {
        /// Output file path (defaults to stdout)
        #[arg(long, short)]
        output: Option<String>,
        /// Output format
        #[arg(long, short, value_enum, default_value = "tsv")]
        format: ExportFormat,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum ExportFormat {
    Tsv,
    Json,
}

#[derive(Subcommand)]
enum ValidateTarget {
    /// Validate a song entry structure
    SongEntry {
        /// Address of the song entry (hex, e.g., 0x1431B08A0)
        #[arg(long)]
        address: String,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
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
            entry_size,
        }) => run_scan_mode(offsets_file.as_deref(), pid, range, tsv.as_deref(), output.as_deref(), entry_size),
        Some(Command::Explore { address, pid }) => {
            let addr = u64::from_str_radix(address.trim_start_matches("0x").trim_start_matches("0X"), 16)?;
            run_explore_mode(addr, pid)
        }
        Some(Command::Hexdump { address, size, ascii, pid }) => {
            let addr = parse_hex_address(&address)?;
            run_hexdump_mode(addr, size, ascii, pid)
        }
        Some(Command::Search { string, i32, i16, pattern, limit, pid }) => {
            run_search_mode(string, i32, i16, pattern, limit, pid)
        }
        Some(Command::Offset { from, to }) => {
            run_offset_mode(&from, &to)
        }
        Some(Command::Validate { target }) => {
            run_validate_mode(target)
        }
        Some(Command::Export { output, format, pid }) => {
            run_export_mode(output.as_deref(), format, pid)
        }
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
        if let Some(&first_addr) = addresses.first() {
            if addresses.len() >= 10 {
                // Try to count songs with this structure size
                let count = count_songs_with_size(reader, first_addr, *delta);
                println!("        -> Starting at 0x{:X}: {} consecutive songs", first_addr, count);
            }
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

/// Run the memory explore command
fn run_explore_mode(base_addr: u64, pid: Option<u32>) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Reflux-RS {} - Memory Explore Mode", current_version);

    let process = if let Some(pid) = pid {
        ProcessHandle::open(pid)?
    } else {
        ProcessHandle::find_and_open()?
    };

    println!("Found process (PID: {}, Base: 0x{:X})", process.pid, process.base_address);
    let reader = MemoryReader::new(&process);

    const ENTRY_SIZE: u64 = 0x3F0; // 1008 bytes
    const METADATA_OFFSET: u64 = 0x7E0; // 2016 bytes

    // Analyze entry states
    println!();
    println!("=== Entry State Analysis at 0x{:X} ===", base_addr);
    println!("Entry size: 0x{:X} ({} bytes)", ENTRY_SIZE, ENTRY_SIZE);
    println!("Metadata offset: 0x{:X} ({} bytes)", METADATA_OFFSET, METADATA_OFFSET);

    let max_entries = 2000u64;
    let mut has_title = 0u32;
    let mut has_valid_meta = 0u32;
    let mut has_both = 0u32;
    let mut title_only = 0u32;
    let mut meta_only = 0u32;
    let mut empty = 0u32;
    let mut read_errors = 0u32;

    let mut found_songs: Vec<(u64, u32, i32, String)> = Vec::new();

    for i in 0..max_entries {
        let text_addr = base_addr + i * ENTRY_SIZE;
        let meta_addr = text_addr + METADATA_OFFSET;

        // Read title
        let title = match reader.read_bytes(text_addr, 64) {
            Ok(bytes) => {
                let len = bytes.iter().position(|&b| b == 0).unwrap_or(64);
                if len > 0 && bytes[0] != 0 {
                    let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(&bytes[..len]);
                    let t = decoded.trim();
                    if !t.is_empty() && t.chars().next().is_some_and(|c| c.is_ascii_graphic() || !c.is_ascii()) {
                        Some(t.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Err(_) => {
                read_errors += 1;
                continue;
            }
        };

        // Read metadata
        let (song_id, folder) = match reader.read_bytes(meta_addr, 8) {
            Ok(bytes) => {
                let id = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let folder = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                (id, folder)
            }
            Err(_) => {
                read_errors += 1;
                continue;
            }
        };

        let valid_meta = song_id >= 1000 && song_id <= 90000 && folder >= 1 && folder <= 200;

        // Debug: Look for song_id=9003 regardless of filter
        if song_id == 9003 {
            println!("*** FOUND song_id=9003 (metadata) at entry={}, folder={}, title={:?}", i, folder, title);
        }

        // Also check C# style offset (0x270 from entry start for song_id)
        let csharp_id_offset = 624u64; // 256 + 368 = 0x270
        if let Ok(csharp_id) = reader.read_i32(text_addr + csharp_id_offset) {
            if csharp_id == 9003 {
                // Read difficulty levels at offset 288 (0x120)
                let levels = reader.read_bytes(text_addr + 288, 10).unwrap_or_default();
                println!("*** FOUND song_id=9003 (C# style) at entry={}, title={:?}, levels={:?}", i, title, levels);
            }
        }

        // Debug: Look for title containing "fun"
        if let Some(ref t) = title {
            if t.to_lowercase().contains("fun") {
                println!("*** FOUND title containing 'fun' at entry={}, id={}, folder={}, title={:?}", i, song_id, folder, t);
            }
        }

        match (title.is_some(), valid_meta) {
            (true, true) => {
                has_title += 1;
                has_valid_meta += 1;
                has_both += 1;
                found_songs.push((i, song_id as u32, folder, title.unwrap()));
            }
            (true, false) => {
                has_title += 1;
                title_only += 1;
            }
            (false, true) => {
                has_valid_meta += 1;
                meta_only += 1;
            }
            (false, false) => {
                empty += 1;
            }
        }
    }

    println!();
    println!("=== Statistics (first {} entries) ===", max_entries);
    println!("  Entries with valid title:    {:5}", has_title);
    println!("  Entries with valid metadata: {:5}", has_valid_meta);
    println!("  Entries with both:           {:5}", has_both);
    println!("  Title only (no valid meta):  {:5}", title_only);
    println!("  Metadata only (no title):    {:5}", meta_only);
    println!("  Empty entries:               {:5}", empty);
    println!("  Read errors:                 {:5}", read_errors);

    println!();
    println!("=== Found songs with title + valid metadata ({} total) ===", found_songs.len());
    for (i, (idx, song_id, folder, title)) in found_songs.iter().take(30).enumerate() {
        println!(
            "  [{:3}] entry={:4}, id={:5}, folder={:3}, title={:?}",
            i, idx, song_id, folder, title
        );
    }
    if found_songs.len() > 30 {
        println!("  ... and {} more", found_songs.len() - 30);
    }

    // Check if entries are contiguous or scattered
    if found_songs.len() >= 2 {
        println!();
        println!("=== Entry distribution ===");
        let indices: Vec<u64> = found_songs.iter().map(|(idx, _, _, _)| *idx).collect();
        let min_idx = *indices.iter().min().unwrap();
        let max_idx = *indices.iter().max().unwrap();
        println!("  Entry range: {} to {} (span: {})", min_idx, max_idx, max_idx - min_idx + 1);
        println!("  Density: {:.1}% of entries in range have songs",
            100.0 * found_songs.len() as f64 / (max_idx - min_idx + 1) as f64);
    }

    // Check first entry (5.1.1.) with both old and new offsets
    println!();
    println!("=== Analyzing first entry (5.1.1.) structure ===");
    if let Ok(data) = reader.read_bytes(base_addr, 1200) {
        println!("  Reading from 0x{:X}:", base_addr);

        // Check title
        let title_len = data.iter().take(64).position(|&b| b == 0).unwrap_or(64);
        let (title, _, _) = encoding_rs::SHIFT_JIS.decode(&data[..title_len]);
        println!("    title at 0: {:?}", title.trim());

        // Check OLD offsets (C# style)
        let old_levels = &data[288..298];
        let old_song_id = i32::from_le_bytes([data[624], data[625], data[626], data[627]]);
        println!("    OLD: song_id at 624 = {}, levels at 288 = {:?}", old_song_id, old_levels);

        // Check NEW offsets (discovered from 'fun')
        let new_levels = &data[480..490];
        let new_song_id = i32::from_le_bytes([data[816], data[817], data[818], data[819]]);
        println!("    NEW: song_id at 816 = {}, levels at 480 = {:?}", new_song_id, new_levels);

        // Dump some key offsets to understand structure
        for offset in [256, 320, 384, 448, 512, 576, 640, 704, 768, 832, 896, 960, 1024, 1088].iter() {
            if *offset + 64 <= 1200 {
                let str_bytes = &data[*offset..*offset + 64];
                let len = str_bytes.iter().position(|&b| b == 0).unwrap_or(64);
                if len > 0 && str_bytes[0] >= 0x20 && str_bytes[0] < 0x80 {
                    let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(&str_bytes[..len]);
                    println!("    offset {}: {:?}", offset, decoded.trim());
                }
            }
        }
    }

    // Try scanning with NEW offsets
    println!();
    println!("=== Scanning with NEW offsets (song_id at 816) ===");
    const NEW_ENTRY_SIZE: u64 = 1200; // Hypothesized new entry size
    let new_max_entries = (0x800000u64 / NEW_ENTRY_SIZE).min(2000);
    let mut found_with_new = Vec::new();

    for i in 0..new_max_entries {
        let entry_addr = base_addr + i * NEW_ENTRY_SIZE;
        if let Ok(data) = reader.read_bytes(entry_addr, 1200) {
            // Check for valid title
            let title_len = data.iter().take(64).position(|&b| b == 0).unwrap_or(64);
            if title_len == 0 || data[0] < 0x20 {
                continue;
            }
            let (title, _, _) = encoding_rs::SHIFT_JIS.decode(&data[..title_len]);
            let title = title.trim();
            if title.is_empty() {
                continue;
            }

            // Read with NEW offsets
            let song_id = i32::from_le_bytes([data[816], data[817], data[818], data[819]]);
            let levels = &data[480..490];

            if song_id >= 1000 && song_id <= 50000 {
                found_with_new.push((i, song_id, title.to_string(), levels.to_vec()));
                if song_id == 9003 {
                    println!("  *** FOUND song_id=9003: entry={}, title={:?}, levels={:?}", i, title, levels);
                }
            }
        }
    }
    println!("  Found {} songs with new offsets", found_with_new.len());
    for (i, (idx, id, title, levels)) in found_with_new.iter().take(10).enumerate() {
        println!("    [{:2}] entry={:4}, id={:5}, title={:?}, levels={:?}", i, idx, id, title, levels);
    }

    // Check currentSong offset (0x1428382d0) and surrounding area
    let current_song_addr = 0x1428382d0u64;
    println!();
    println!("=== Current Song Info at 0x{:X} ===", current_song_addr);
    if let Ok(bytes) = reader.read_bytes(current_song_addr, 128) {
        let song_id = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let difficulty = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        println!("  song_id (offset 0): {}", song_id);
        println!("  difficulty (offset 4): {}", difficulty);
        println!("  raw bytes 0-63: {:02X?}", &bytes[..64]);
        println!("  raw bytes 64-127: {:02X?}", &bytes[64..128]);

        // Look for pointers (values that look like addresses)
        for i in (0..120).step_by(8) {
            let val = u64::from_le_bytes([
                bytes[i], bytes[i+1], bytes[i+2], bytes[i+3],
                bytes[i+4], bytes[i+5], bytes[i+6], bytes[i+7],
            ]);
            if val > 0x140000000 && val < 0x150000000 {
                println!("  Potential pointer at offset {}: 0x{:X}", i, val);
                // Try to read what's at that address
                if let Ok(target_bytes) = reader.read_bytes(val, 64) {
                    let len = target_bytes.iter().position(|&b| b == 0).unwrap_or(64);
                    if len > 0 && target_bytes[0] >= 0x20 {
                        let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(&target_bytes[..len]);
                        println!("    -> String: {:?}", decoded.trim());
                    }
                }
            }
        }
    }

    // Search for "fun" string in memory (around song_list area)
    println!();
    println!("=== Searching for 'fun' string in memory ===");
    let search_start = base_addr;
    let search_size = 0x800000u64; // 8MB
    let chunk_size = 0x10000usize; // 64KB chunks

    let fun_pattern = b"fun\x00"; // "fun" followed by null terminator
    let mut found_fun = Vec::new();

    for chunk_start in (0..search_size).step_by(chunk_size) {
        let addr = search_start + chunk_start;
        if let Ok(chunk) = reader.read_bytes(addr, chunk_size) {
            for i in 0..(chunk_size - 4) {
                if &chunk[i..i+4] == fun_pattern {
                    found_fun.push(addr + i as u64);
                }
            }
        }
    }

    println!("  Found {} occurrences of 'fun\\0'", found_fun.len());
    // Only analyze the first "fun" as it appears to be the title
    if let Some(addr) = found_fun.first() {
        println!("  Analyzing first 'fun' at 0x{:X} as entry start:", addr);

        // Read a larger buffer to analyze the structure
        if let Ok(data) = reader.read_bytes(*addr, 1024) {
            // Dump strings at key offsets
            for (name, offset) in [
                ("title (0)", 0usize),
                ("title_en (64)", 64),
                ("genre (128)", 128),
                ("artist (192)", 192),
                ("unknown (256)", 256),
                ("unknown (320)", 320),
                ("unknown (384)", 384),
                ("unknown (448)", 448),
                ("unknown (512)", 512),
            ].iter() {
                let str_bytes = &data[*offset..*offset + 64];
                let len = str_bytes.iter().position(|&b| b == 0).unwrap_or(64);
                if len > 0 && str_bytes[0] >= 0x20 {
                    let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(&str_bytes[..len]);
                    println!("      {}: {:?}", name, decoded.trim());
                } else {
                    println!("      {}: (empty or binary)", name);
                }
            }

            // Check for levels-like data (10 consecutive small bytes)
            println!("      Scanning for levels pattern (10 bytes, values 0-12):");
            for offset in (256..900).step_by(8) {
                let slice = &data[offset..offset + 10];
                if slice.iter().all(|&b| b <= 12) && slice.iter().any(|&b| b > 0) {
                    println!("        offset {}: {:?}", offset, slice);
                }
            }
        }

        // Check larger area for song_id
        if let Ok(wide_data) = reader.read_bytes(*addr, 1024) {
            println!("      Scanning for song_id=9003 in entry:");
            let target_bytes: [u8; 4] = 9003u32.to_le_bytes();
            for j in 0..1020 {
                if &wide_data[j..j+4] == &target_bytes {
                    println!("        *** song_id=9003 at offset {} ***", j);
                }
            }
        }
    }

    // Search for song_id=9003 (0x232B) as a 4-byte value in memory
    println!();
    println!("=== Searching for song_id=9003 (0x232B) as 4-byte value ===");
    let target_id: u32 = 9003;
    let target_bytes = target_id.to_le_bytes();
    let search_size = 0x800000usize; // 8MB

    if let Ok(data) = reader.read_bytes(base_addr, search_size) {
        let mut found_locations = Vec::new();
        for i in 0..(search_size - 4) {
            if &data[i..i+4] == &target_bytes {
                found_locations.push(base_addr + i as u64);
            }
        }

        println!("  Found {} occurrences of 0x{:08X} ({})", found_locations.len(), target_id, target_id);
        for (i, addr) in found_locations.iter().take(20).enumerate() {
            let offset_from_base = addr - base_addr;
            println!("  [{}] 0x{:X} (base+0x{:X})", i, addr, offset_from_base);

            // Read context around this location
            let context_start = addr.saturating_sub(64);
            if reader.read_bytes(context_start, 256).is_ok() {
                // Check for readable strings nearby
                let mut strings_found = Vec::new();

                // Check various offsets for strings
                for string_offset in [-624i64, -560, -432, -288, -192, -128, -64, 0, 64].iter() {
                    let check_addr = (*addr as i64 + string_offset) as u64;
                    if let Ok(str_bytes) = reader.read_bytes(check_addr, 64) {
                        let len = str_bytes.iter().position(|&b| b == 0).unwrap_or(64);
                        if len > 2 && str_bytes[0] >= 0x20 && str_bytes[0] < 0x80 {
                            let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(&str_bytes[..len]);
                            let s = decoded.trim();
                            if !s.is_empty() && s.len() >= 2 {
                                strings_found.push((string_offset, s.to_string()));
                            }
                        }
                    }
                }

                if !strings_found.is_empty() {
                    for (off, s) in &strings_found {
                        println!("      string at offset {}: {:?}", off, s);
                    }
                }

                // Also check if this might be a song entry
                // If 9003 is at offset 624, entry start would be addr - 624
                let potential_entry_start = addr.saturating_sub(624);
                if let Ok(entry) = reader.read_bytes(potential_entry_start, 1008) {
                    let title_len = entry.iter().take(64).position(|&b| b == 0).unwrap_or(64);
                    if title_len > 0 && entry[0] >= 0x20 {
                        let (title, _, _) = encoding_rs::SHIFT_JIS.decode(&entry[..title_len]);
                        let levels = &entry[288..298];
                        println!("      -> if at offset 624: entry=0x{:X}, title={:?}, levels={:?}",
                                 potential_entry_start, title.trim(), levels);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Parse a hex address string (with or without 0x prefix)
fn parse_hex_address(s: &str) -> Result<u64> {
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(s, 16).map_err(|e| anyhow::anyhow!("Invalid hex address: {}", e))
}

/// Run the hexdump command
fn run_hexdump_mode(address: u64, size: usize, ascii: bool, pid: Option<u32>) -> Result<()> {
    let process = if let Some(pid) = pid {
        ProcessHandle::open(pid)?
    } else {
        ProcessHandle::find_and_open()?
    };

    let reader = MemoryReader::new(&process);
    let bytes = reader.read_bytes(address, size)?;

    println!("Hexdump at 0x{:X} ({} bytes):", address, size);
    println!();

    for (i, chunk) in bytes.chunks(16).enumerate() {
        let offset = i * 16;
        print!("0x{:03X}: ", offset);

        // Hex bytes
        for (j, byte) in chunk.iter().enumerate() {
            if j == 8 {
                print!(" ");
            }
            print!("{:02X} ", byte);
        }

        // Padding for incomplete lines
        if chunk.len() < 16 {
            for j in chunk.len()..16 {
                if j == 8 {
                    print!(" ");
                }
                print!("   ");
            }
        }

        // ASCII representation
        if ascii {
            print!(" |");
            for byte in chunk {
                if *byte >= 0x20 && *byte < 0x7F {
                    print!("{}", *byte as char);
                } else {
                    print!(".");
                }
            }
            for _ in chunk.len()..16 {
                print!(" ");
            }
            print!("|");
        }

        println!();
    }

    Ok(())
}

/// Run the search command
fn run_search_mode(
    string: Option<String>,
    i32_val: Option<i32>,
    i16_val: Option<i16>,
    pattern: Option<String>,
    limit: usize,
    pid: Option<u32>,
) -> Result<()> {
    let process = if let Some(pid) = pid {
        ProcessHandle::open(pid)?
    } else {
        ProcessHandle::find_and_open()?
    };

    println!("Found process (PID: {}, Base: 0x{:X})", process.pid, process.base_address);

    let reader = MemoryReader::new(&process);

    // Determine search pattern
    let (search_bytes, wildcard_mask): (Vec<u8>, Vec<bool>) = if let Some(ref s) = string {
        // Encode string as Shift-JIS
        let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode(s);
        let bytes = encoded.to_vec();
        let mask = vec![false; bytes.len()];
        println!("Searching for string: {:?} ({} bytes, Shift-JIS)", s, bytes.len());
        (bytes, mask)
    } else if let Some(val) = i32_val {
        let bytes = val.to_le_bytes().to_vec();
        let mask = vec![false; 4];
        println!("Searching for i32: {} (0x{:08X})", val, val as u32);
        (bytes, mask)
    } else if let Some(val) = i16_val {
        let bytes = val.to_le_bytes().to_vec();
        let mask = vec![false; 2];
        println!("Searching for i16: {} (0x{:04X})", val, val as u16);
        (bytes, mask)
    } else if let Some(ref pat) = pattern {
        // Parse byte pattern (e.g., "00 04 07 0A" or "00 ?? 07")
        let parts: Vec<&str> = pat.split_whitespace().collect();
        let mut bytes = Vec::new();
        let mut mask = Vec::new();
        for part in parts {
            if part == "??" {
                bytes.push(0);
                mask.push(true); // wildcard
            } else {
                let byte = u8::from_str_radix(part, 16)
                    .map_err(|_| anyhow::anyhow!("Invalid hex byte: {}", part))?;
                bytes.push(byte);
                mask.push(false);
            }
        }
        println!("Searching for pattern: {} ({} bytes)", pat, bytes.len());
        (bytes, mask)
    } else {
        bail!("No search pattern specified. Use --string, --i32, --i16, or --pattern");
    };

    // Search in memory
    let search_start = process.base_address + 0x1000000; // Start 16MB into the module
    let search_end = process.base_address + (process.module_size as u64).min(0x5000000);
    let chunk_size: usize = 4 * 1024 * 1024; // 4MB chunks

    println!("Search range: 0x{:X} - 0x{:X}", search_start, search_end);
    println!();

    let mut found: Vec<u64> = Vec::new();
    let mut offset = 0u64;

    while search_start + offset < search_end && found.len() < limit {
        let addr = search_start + offset;
        let read_size = chunk_size.min((search_end - addr) as usize);

        if let Ok(buffer) = reader.read_bytes(addr, read_size) {
            for i in 0..=(buffer.len().saturating_sub(search_bytes.len())) {
                let mut matches = true;
                for (j, &byte) in search_bytes.iter().enumerate() {
                    if !wildcard_mask[j] && buffer[i + j] != byte {
                        matches = false;
                        break;
                    }
                }
                if matches {
                    let found_addr = addr + i as u64;
                    found.push(found_addr);

                    println!("[{}] 0x{:X}", found.len(), found_addr);

                    // Show context (32 bytes)
                    if let Ok(context) = reader.read_bytes(found_addr, 32.min(buffer.len() - i)) {
                        print!("     ");
                        for byte in &context[..16.min(context.len())] {
                            print!("{:02X} ", byte);
                        }
                        println!();
                    }

                    if found.len() >= limit {
                        break;
                    }
                }
            }
        }

        offset += chunk_size as u64;
    }

    println!();
    println!("Found {} result(s)", found.len());
    if found.len() >= limit {
        println!("(limit reached, use --limit to increase)");
    }

    Ok(())
}

/// Run the offset command
fn run_offset_mode(from: &str, to: &str) -> Result<()> {
    let from_addr = parse_hex_address(from)?;
    let to_addr = parse_hex_address(to)?;

    let diff = if to_addr >= from_addr {
        to_addr - from_addr
    } else {
        from_addr - to_addr
    };

    let sign = if to_addr >= from_addr { "" } else { "-" };

    println!("From: 0x{:X}", from_addr);
    println!("To:   0x{:X}", to_addr);
    println!();
    println!("Offset: {}{} (0x{:X})", sign, diff, diff);

    Ok(())
}

/// Run the validate command
fn run_validate_mode(target: ValidateTarget) -> Result<()> {
    match target {
        ValidateTarget::SongEntry { address, pid } => {
            let addr = parse_hex_address(&address)?;
            run_validate_song_entry(addr, pid)
        }
    }
}

/// Validate a song entry structure
fn run_validate_song_entry(address: u64, pid: Option<u32>) -> Result<()> {
    let process = if let Some(pid) = pid {
        ProcessHandle::open(pid)?
    } else {
        ProcessHandle::find_and_open()?
    };

    let reader = MemoryReader::new(&process);

    // Read entry data (1200 bytes for new structure)
    const ENTRY_SIZE: usize = 1200;
    let data = reader.read_bytes(address, ENTRY_SIZE)?;

    println!("=== Song Entry Validation ===");
    println!("Address: 0x{:X}", address);
    println!("Entry size: {} bytes (0x{:X})", ENTRY_SIZE, ENTRY_SIZE);
    println!();
    println!("Fields:");

    // Helper to decode Shift-JIS string
    let decode_string = |offset: usize, max_len: usize| -> String {
        let slice = &data[offset..offset + max_len];
        let len = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
        if len == 0 {
            return "(empty)".to_string();
        }
        let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(&slice[..len]);
        decoded.trim().to_string()
    };

    // Helper to check if field looks valid
    let check_string = |s: &str| -> &str {
        if s == "(empty)" || s.chars().any(|c| c.is_control() && c != '\n' && c != '\r') {
            "?"
        } else {
            "✓"
        }
    };

    // Title at offset 0
    let title = decode_string(0, 64);
    println!("  title     @    0: {:?} {}", title, check_string(&title));

    // Title English at offset 64
    let title_en = decode_string(64, 64);
    println!("  title_en  @   64: {:?} {}", title_en, check_string(&title_en));

    // Genre at offset 128
    let genre = decode_string(128, 64);
    println!("  genre     @  128: {:?} {}", genre, check_string(&genre));

    // Artist at offset 192
    let artist = decode_string(192, 64);
    println!("  artist    @  192: {:?} {}", artist, check_string(&artist));

    // Levels at offset 480
    let levels: Vec<u8> = data[480..490].to_vec();
    let levels_valid = levels.iter().all(|&l| l <= 12);
    println!("  levels    @  480: {:?} {}", levels, if levels_valid { "✓" } else { "?" });

    // Song ID at offset 816
    let song_id = i32::from_le_bytes([data[816], data[817], data[818], data[819]]);
    let song_id_valid = song_id >= 1000 && song_id <= 90000;
    println!("  song_id   @  816: {} {}", song_id, if song_id_valid { "✓" } else { "?" });

    // Folder at offset 820
    let folder = i32::from_le_bytes([data[820], data[821], data[822], data[823]]);
    let folder_valid = folder >= 1 && folder <= 200;
    println!("  folder    @  820: {} {}", folder, if folder_valid { "✓" } else { "?" });

    // Total notes at offset 500 (10 x u16)
    let total_notes: Vec<u16> = (0..10)
        .map(|i| {
            let off = 500 + i * 2;
            u16::from_le_bytes([data[off], data[off + 1]])
        })
        .collect();
    println!("  notes     @  500: {:?}", total_notes);

    // BPM at offset 256
    let bpm = decode_string(256, 64);
    println!("  bpm       @  256: {:?}", bpm);

    println!();

    // Overall validation
    let valid = !title.is_empty()
        && title != "(empty)"
        && song_id_valid
        && levels_valid;
    println!(
        "Overall: {}",
        if valid { "Valid song entry" } else { "Invalid or unknown structure" }
    );

    Ok(())
}

/// Export all play data
fn run_export_mode(output: Option<&str>, format: ExportFormat, pid: Option<u32>) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    eprintln!("Reflux-RS {} - Export Mode", current_version);

    // Open process
    let process = if let Some(pid) = pid {
        ProcessHandle::open(pid)?
    } else {
        ProcessHandle::find_and_open()?
    };

    eprintln!(
        "Found process (PID: {}, Base: 0x{:X})",
        process.pid, process.base_address
    );

    let reader = MemoryReader::new(&process);

    // Search for offsets using builtin signatures
    let signatures = builtin_signatures();
    let mut searcher = OffsetSearcher::new(&reader);
    let offsets = searcher.search_all_with_signatures(&signatures)?;

    eprintln!("Offsets detected");

    // Load encoding fixes (optional)
    let encoding_fixes = match EncodingFixes::load("encodingfixes.txt") {
        Ok(ef) => {
            eprintln!("Loaded {} encoding fixes", ef.len());
            Some(ef)
        }
        Err(_) => None,
    };

    // Load song database
    eprintln!("Loading song database...");
    let song_db =
        fetch_song_database_with_fixes(&reader, offsets.song_list, encoding_fixes.as_ref())?;
    eprintln!("Loaded {} songs", song_db.len());

    // Load unlock data
    eprintln!("Loading unlock data...");
    let unlock_db = get_unlock_states(&reader, offsets.unlock_data, &song_db)?;
    eprintln!("Loaded {} unlock entries", unlock_db.len());

    // Load score map
    eprintln!("Loading score data...");
    let score_map = ScoreMap::load_from_memory(&reader, offsets.data_map, &song_db)?;
    eprintln!("Loaded {} score entries", score_map.len());

    // Load custom types (optional, for TSV format)
    let custom_types: HashMap<u32, String> = match CustomTypes::load("customtypes.txt") {
        Ok(ct) => {
            let mut types = HashMap::new();
            for (k, v) in ct.iter() {
                if let Ok(id) = k.parse::<u32>() {
                    types.insert(id, v.clone());
                }
            }
            eprintln!("Loaded {} custom types", types.len());
            types
        }
        Err(_) => HashMap::new(),
    };

    // Generate output based on format
    let content = match format {
        ExportFormat::Tsv => generate_tracker_tsv(&song_db, &unlock_db, &score_map, &custom_types),
        ExportFormat::Json => generate_tracker_json(&song_db, &unlock_db, &score_map)?,
    };

    // Write output
    if let Some(output_path) = output {
        std::fs::write(output_path, &content)?;
        eprintln!("Exported to: {}", output_path);
    } else {
        println!("{}", content);
    }

    Ok(())
}

/// Scan with custom entry size
fn run_custom_entry_size_scan(reader: &MemoryReader, start_addr: u64, range: usize, entry_size: usize) {
    use encoding_rs::SHIFT_JIS;

    println!();
    println!("=== Custom Entry Size Scan ===");
    println!("Entry size: {} bytes (0x{:X})", entry_size, entry_size);
    println!();

    let max_entries = (range / entry_size).min(5000);
    let mut found_songs: Vec<(u64, u32, String, [u8; 10])> = Vec::new();
    let mut consecutive_empty = 0;

    for i in 0..max_entries {
        let entry_addr = start_addr + (i * entry_size) as u64;

        // Read entry
        let data = match reader.read_bytes(entry_addr, entry_size) {
            Ok(d) => d,
            Err(_) => {
                consecutive_empty += 1;
                if consecutive_empty >= 10 {
                    break;
                }
                continue;
            }
        };

        // Try to extract title (offset 0, 64 bytes)
        let title_len = data.iter().take(64).position(|&b| b == 0).unwrap_or(64);
        if title_len == 0 || data[0] < 0x20 {
            consecutive_empty += 1;
            if consecutive_empty >= 20 {
                break;
            }
            continue;
        }

        let (title, _, _) = SHIFT_JIS.decode(&data[..title_len]);
        let title = title.trim();
        if title.is_empty() {
            consecutive_empty += 1;
            if consecutive_empty >= 20 {
                break;
            }
            continue;
        }

        consecutive_empty = 0;

        // Try to find song_id at common offsets
        let mut song_id = 0u32;
        let mut levels = [0u8; 10];

        // Try offsets based on entry size
        let id_offsets: &[usize] = match entry_size {
            1200 => &[816, 624], // New structure, old structure
            1008 => &[624, 816],
            _ => &[624, 816, entry_size - 384, entry_size - 192],
        };

        for &offset in id_offsets {
            if offset + 4 <= entry_size {
                let id = i32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                if id >= 1000 && id <= 90000 {
                    song_id = id as u32;
                    break;
                }
            }
        }

        // Try to find levels
        let level_offsets: &[usize] = match entry_size {
            1200 => &[480, 288],
            1008 => &[288, 480],
            _ => &[288, 480, 256],
        };

        for &offset in level_offsets {
            if offset + 10 <= entry_size {
                let slice = &data[offset..offset + 10];
                if slice.iter().all(|&b| b <= 12) && slice.iter().any(|&b| b > 0) {
                    for (j, &b) in slice.iter().enumerate() {
                        levels[j] = b;
                    }
                    break;
                }
            }
        }

        found_songs.push((entry_addr, song_id, title.to_string(), levels));
    }

    println!("Found {} entries with titles", found_songs.len());
    println!();

    // Display results
    for (i, (addr, id, title, levels)) in found_songs.iter().take(30).enumerate() {
        let id_str = if *id > 0 {
            format!("{:5}", id)
        } else {
            "    ?".to_string()
        };
        println!(
            "[{:3}] 0x{:X}: id={}, levels={:?}, title={:?}",
            i, addr, id_str, levels, title
        );
    }

    if found_songs.len() > 30 {
        println!("... and {} more", found_songs.len() - 30);
    }

    // Statistics
    let with_id = found_songs.iter().filter(|(_, id, _, _)| *id > 0).count();
    let with_levels = found_songs.iter().filter(|(_, _, _, l)| l.iter().any(|&x| x > 0)).count();
    println!();
    println!("Statistics:");
    println!("  Entries with valid song_id: {}", with_id);
    println!("  Entries with valid levels:  {}", with_levels);
}

/// Run the scan command
fn run_scan_mode(
    offsets_file: Option<&str>,
    pid: Option<u32>,
    range: usize,
    tsv_file: Option<&str>,
    output: Option<&str>,
    entry_size: Option<usize>,
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
    if let Some(size) = entry_size {
        // Custom entry size scan
        println!("Scanning with entry size {} bytes from 0x{:X}...", size, offsets.song_list);
        run_custom_entry_size_scan(&reader, offsets.song_list, range, size);
        return Ok(());
    }

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
    let (initial_offsets, offsets_from_file) = if let Some(path) = offsets_file {
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
                // Note: For offsets loaded from file, skip distance-based validation
                // as the relative distances may differ between game versions
                let needs_search = if !reflux.offsets().is_valid() {
                    info!("Invalid offsets detected (some offsets are zero)");
                    true
                } else if offsets_from_file {
                    // For file-loaded offsets, just verify memory is readable
                    let searcher = OffsetSearcher::new(&reader);
                    if searcher.validate_basic_memory_access(reflux.offsets()) {
                        debug!("File-loaded offsets: basic memory access validated");
                        false
                    } else {
                        info!("File-loaded offsets: memory access failed. Attempting signature search...");
                        true
                    }
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
