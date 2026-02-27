//! Navigate command — move cursor to a target song on the select screen.

use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Result, bail};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use infst::{
    Difficulty, MemoryReader, NavigationResult, OffsetSearcher, SongInfo, SongNavigator,
    builtin_signatures, fetch_song_database_bulk, input::window,
};

use crate::cli_utils;

pub fn run(
    target: &str,
    difficulty: Option<&str>,
    max_steps: u32,
    key_delay_ms: u64,
    pid: Option<u32>,
) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    eprintln!("infst {} - Navigate Mode", current_version);

    // Parse difficulty if provided
    let target_difficulty: Option<Difficulty> = match difficulty {
        Some(d) => {
            let parsed: Difficulty = d.to_uppercase().parse().map_err(|_| {
                anyhow::anyhow!(
                    "Invalid difficulty: {}. Use SPN, SPH, SPA, SPL, DPN, DPH, DPA, DPL",
                    d
                )
            })?;
            Some(parsed)
        }
        None => None,
    };

    let process = cli_utils::open_process(pid)?;
    eprintln!(
        "Found process (PID: {}, Base: 0x{:X})",
        process.pid, process.base_address
    );

    let reader = MemoryReader::new(&process);
    let signatures = builtin_signatures();
    let mut searcher = OffsetSearcher::new(&reader);
    let offsets = searcher.search_all_with_signatures(&signatures)?;

    if offsets.current_song == 0 || offsets.song_list == 0 {
        bail!("Required offsets (current_song, song_list) not found");
    }
    eprintln!("Offsets detected");

    // Load song database
    eprintln!("Loading song database...");
    let song_db = fetch_song_database_bulk(&reader, offsets.song_list)?;
    if song_db.is_empty() {
        bail!("Song database is empty — are you on the song select screen?");
    }
    eprintln!("Loaded {} songs", song_db.len());

    // Fuzzy search
    let song_id = fuzzy_select_song(target, &song_db)?;
    let song_title = song_db
        .get(&song_id)
        .map(|s| s.title.to_string())
        .unwrap_or_else(|| format!("(id={})", song_id));

    eprintln!("Target: {} (song_id={})", song_title, song_id);

    // Focus game window
    #[cfg(target_os = "windows")]
    {
        eprintln!("Focusing game window...");
        let hwnd = window::find_window_by_pid(process.pid)?;
        window::ensure_foreground(hwnd)?;
        // Brief pause to let the window come to foreground
        std::thread::sleep(Duration::from_millis(200));
    }

    // Navigate
    let shutdown = AtomicBool::new(false);
    let nav = SongNavigator::new(&reader, offsets.current_song)
        .with_key_delay(Duration::from_millis(key_delay_ms));

    eprintln!("Navigating (max {} steps)...", max_steps);
    let result = nav.navigate_to_song(song_id, max_steps, &shutdown)?;

    match &result {
        NavigationResult::Success { steps } => {
            eprintln!("Reached target in {} steps", steps);
        }
        NavigationResult::NotFound { steps } => {
            bail!(
                "Song not found in current list after {} steps. Is the song visible in the current folder/sort?",
                steps
            );
        }
        NavigationResult::Timeout { steps } => {
            bail!(
                "Navigation timed out after {} steps (max={})",
                steps,
                max_steps
            );
        }
        NavigationResult::Cancelled { steps } => {
            eprintln!("Navigation cancelled after {} steps", steps);
            return Ok(());
        }
    }

    // Difficulty selection
    if let Some(diff) = target_difficulty {
        eprintln!("Selecting difficulty: {}...", diff);
        let ok = nav.select_difficulty(diff as u8, 20, &shutdown)?;
        if !ok {
            eprintln!("Warning: could not confirm difficulty change to {}", diff);
        }
    }

    // Confirm selection
    eprintln!("Confirming selection...");
    nav.confirm_selection()?;

    eprintln!("Done!");
    Ok(())
}

/// Fuzzy-match the query against the song database and let the user pick.
fn fuzzy_select_song(
    query: &str,
    song_db: &std::collections::HashMap<u32, SongInfo>,
) -> Result<u32> {
    let matcher = SkimMatcherV2::default();

    // Score every song title against the query
    let mut scored: Vec<(i64, u32, Arc<str>)> = song_db
        .values()
        .filter_map(|song| {
            // Match against both Japanese title and English title
            let score_jp = matcher.fuzzy_match(&song.title, query).unwrap_or(0);
            let score_en = matcher.fuzzy_match(&song.title_english, query).unwrap_or(0);
            let best = score_jp.max(score_en);
            if best > 0 {
                Some((best, song.id, song.title.clone()))
            } else {
                None
            }
        })
        .collect();

    if scored.is_empty() {
        bail!("No songs matching \"{}\"", query);
    }

    // Sort by score descending
    scored.sort_by(|a, b| b.0.cmp(&a.0));

    // If the top match is significantly better, auto-select
    if scored.len() == 1 || (scored.len() > 1 && scored[0].0 > scored[1].0 * 2) {
        let (_, id, title) = &scored[0];
        eprintln!("Auto-selected: {} (score={})", title, scored[0].0);
        return Ok(*id);
    }

    // Show top candidates
    let display_count = scored.len().min(10);
    eprintln!("\nMatching songs:");
    for (i, (score, id, title)) in scored.iter().take(display_count).enumerate() {
        eprintln!("  {}: {} (id={}, score={})", i + 1, title, id, score);
    }

    // Prompt user
    eprint!("\nSelect [1-{}]: ", display_count);
    io::stderr().flush()?;

    let stdin = io::stdin();
    let line = stdin
        .lock()
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No input"))??;

    let choice: usize = line
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid selection: {}", line.trim()))?;

    if choice < 1 || choice > display_count {
        bail!("Selection out of range: {}", choice);
    }

    Ok(scored[choice - 1].1)
}
