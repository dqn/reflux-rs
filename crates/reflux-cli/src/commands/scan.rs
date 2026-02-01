//! Scan command implementation.
//!
//! Scans the song list memory region to extract song information. Can optionally
//! validate against a TSV file to verify detected songs match expected data.
//!
//! Supports custom entry sizes for investigating different structure layouts.

use anyhow::Result;
use reflux_core::{
    MemoryReader, OffsetSearcher, ProcessHandle, ReadMemory, ScanResult, builtin_signatures,
    load_offsets,
};
use tracing::warn;

/// Run the scan command
pub fn run(
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
        match reflux_core::chart::load_song_database_from_tsv(tsv_path) {
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
        println!(
            "Scanning with entry size {} bytes from 0x{:X}...",
            size, offsets.song_list
        );
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

/// Scan with custom entry size
fn run_custom_entry_size_scan(
    reader: &MemoryReader,
    start_addr: u64,
    range: usize,
    entry_size: usize,
) {
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
                let id = i32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                if (1000..=90000).contains(&id) {
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
    let with_levels = found_songs
        .iter()
        .filter(|(_, _, _, l)| l.iter().any(|&x| x > 0))
        .count();
    println!();
    println!("Statistics:");
    println!("  Entries with valid song_id: {}", with_id);
    println!("  Entries with valid levels:  {}", with_levels);
}
