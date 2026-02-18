//! Sync command for reading game memory and uploading directly to the web service.

use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;
use std::{fs, path::Path};

use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use infst::{
    MemoryReader, OffsetSearcher, ScoreMap, chart::Difficulty, fetch_song_database_bulk,
    score::Lamp,
};
use serde::{Deserialize, Serialize};

use super::upload::resolve_credentials;
use crate::cli_utils;

#[derive(Serialize, Clone)]
struct LampEntry {
    #[serde(rename = "songId")]
    song_id: u32,
    difficulty: String,
    lamp: String,
    #[serde(rename = "exScore")]
    ex_score: u32,
    #[serde(rename = "missCount")]
    miss_count: u32,
}

const ALL_DIFFICULTIES: [Difficulty; 10] = [
    Difficulty::SpB,
    Difficulty::SpN,
    Difficulty::SpH,
    Difficulty::SpA,
    Difficulty::SpL,
    Difficulty::DpB,
    Difficulty::DpN,
    Difficulty::DpH,
    Difficulty::DpA,
    Difficulty::DpL,
];

// --- Sync cache for differential sync ---

const SYNC_CACHE_FILE: &str = ".infst-sync-cache.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CachedEntry {
    lamp: String,
    ex_score: u32,
    miss_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct SyncCache {
    entries: HashMap<String, CachedEntry>,
}

impl SyncCache {
    fn load() -> Option<Self> {
        let path = Path::new(SYNC_CACHE_FILE);
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn save(&self) {
        let Ok(content) = serde_json::to_string(self) else {
            return;
        };
        let _ = fs::write(SYNC_CACHE_FILE, content);
    }

    fn make_key(song_id: u32, difficulty: &str) -> String {
        format!("{}:{}", song_id, difficulty)
    }
}

pub fn run(endpoint: Option<&str>, token: Option<&str>, pid: Option<u32>) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    eprintln!("infst {} - Sync Mode", current_version);

    // Resolve credentials
    let (resolved_endpoint, resolved_token) = resolve_credentials(endpoint, token)?;

    let process = cli_utils::open_process(pid)?;

    eprintln!(
        "Found process (PID: {}, Base: 0x{:X})",
        process.pid, process.base_address
    );

    let reader = MemoryReader::new(&process);
    // Search only required offsets (song list/data map)
    let mut searcher = OffsetSearcher::new(&reader);
    let offsets = searcher.search_sync_offsets()?;
    eprintln!("Offsets detected");

    // Load song database (bulk read for fewer syscalls)
    eprintln!("Loading song database...");
    let song_db = fetch_song_database_bulk(&reader, offsets.song_list)?;
    eprintln!("Loaded {} songs", song_db.len());

    // Load score map
    eprintln!("Loading score data...");
    let score_map = ScoreMap::load_from_memory(&reader, offsets.data_map, &song_db)?;
    eprintln!("Loaded {} score entries", score_map.len());

    // Build LampEntry list directly from memory data
    let mut entries: Vec<LampEntry> = Vec::new();

    for (song_id, song_info) in &song_db {
        let score_data = match score_map.get(*song_id) {
            Some(data) => data,
            None => continue,
        };

        for &diff in &ALL_DIFFICULTIES {
            let diff_idx = diff as usize;

            // Skip charts with no notes (chart doesn't exist)
            if song_info.total_notes[diff_idx] == 0 {
                continue;
            }

            // Sync only level 11/12 charts.
            let level = song_info.levels[diff_idx];
            if level != 11 && level != 12 {
                continue;
            }

            let lamp = score_data.get_lamp(diff);

            // Skip NO PLAY
            if lamp == Lamp::NoPlay {
                continue;
            }

            entries.push(LampEntry {
                song_id: *song_id,
                difficulty: diff.short_name().to_string(),
                lamp: lamp.short_name().to_string(),
                ex_score: score_data.get_score(diff),
                miss_count: score_data.miss_count[diff_idx].unwrap_or(0),
            });
        }
    }

    if entries.is_empty() {
        println!("No play data found to sync.");
        return Ok(());
    }

    // Differential sync: filter to changed entries only
    let cache = SyncCache::load();
    let entries_to_send: Vec<LampEntry> = if let Some(ref cache) = cache {
        entries
            .iter()
            .filter(|e| {
                let key = SyncCache::make_key(e.song_id, &e.difficulty);
                match cache.entries.get(&key) {
                    Some(cached) => {
                        cached.lamp != e.lamp
                            || cached.ex_score != e.ex_score
                            || cached.miss_count != e.miss_count
                    }
                    None => true, // New entry
                }
            })
            .cloned()
            .collect()
    } else {
        entries.clone()
    };

    if entries_to_send.is_empty() {
        println!("No changes detected since last sync.");
        return Ok(());
    }

    eprintln!(
        "Uploading {} entries ({} total, {} changed)...",
        entries_to_send.len(),
        entries.len(),
        entries_to_send.len()
    );

    // POST /api/lamps/bulk with gzip compression
    let url = format!("{}/api/lamps/bulk", resolved_endpoint.trim_end_matches('/'));
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    let agent: ureq::Agent = config.into();

    let body = serde_json::json!({ "entries": entries_to_send });
    let json_bytes = serde_json::to_vec(&body).context("Failed to serialize JSON")?;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&json_bytes)
        .context("Failed to compress data")?;
    let compressed = encoder.finish().context("Failed to finish compression")?;

    eprintln!(
        "Payload: {} bytes -> {} bytes (gzip)",
        json_bytes.len(),
        compressed.len()
    );

    let response = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {}", resolved_token))
        .header("Content-Type", "application/json")
        .header("Content-Encoding", "gzip")
        .send(compressed.as_slice())
        .context("Failed to upload data")?;

    println!("Sync complete (status: {})", response.status());
    println!("Synced {} entries.", entries_to_send.len());

    // Update cache with all current entries
    let mut new_cache = SyncCache {
        entries: HashMap::new(),
    };
    for e in &entries {
        let key = SyncCache::make_key(e.song_id, &e.difficulty);
        new_cache.entries.insert(
            key,
            CachedEntry {
                lamp: e.lamp.clone(),
                ex_score: e.ex_score,
                miss_count: e.miss_count,
            },
        );
    }
    new_cache.save();

    Ok(())
}
