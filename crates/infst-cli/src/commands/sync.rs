//! Sync command for reading game memory and uploading directly to the web service.

use anyhow::{Context, Result};
use infst::{
    MemoryReader, ScoreMap, chart::Difficulty, fetch_song_database, get_unlock_states, score::Lamp,
};
use serde::Serialize;
use std::time::Duration;

use super::upload::resolve_credentials;
use crate::cli_utils;

#[derive(Serialize)]
struct LampEntry {
    #[serde(rename = "infinitasTitle")]
    infinitas_title: String,
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
    let offsets = cli_utils::search_offsets(&reader)?;
    eprintln!("Offsets detected");

    // Load song database
    eprintln!("Loading song database...");
    let song_db = fetch_song_database(&reader, offsets.song_list)?;
    eprintln!("Loaded {} songs", song_db.len());

    // Load unlock data (needed for title resolution)
    eprintln!("Loading unlock data...");
    let unlock_db = get_unlock_states(&reader, offsets.unlock_data, &song_db)?;
    eprintln!("Loaded {} unlock entries", unlock_db.len());

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

            let lamp = score_data.get_lamp(diff);

            // Skip NO PLAY
            if lamp == Lamp::NoPlay {
                continue;
            }

            entries.push(LampEntry {
                infinitas_title: song_info.title.to_string(),
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

    eprintln!("Uploading {} entries...", entries.len());

    // POST /api/lamps/bulk
    let url = format!("{}/api/lamps/bulk", resolved_endpoint.trim_end_matches('/'));
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    let agent: ureq::Agent = config.into();

    let body = serde_json::json!({ "entries": entries });
    let response = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {}", resolved_token))
        .send_json(&body)
        .context("Failed to upload data")?;

    println!("Sync complete (status: {})", response.status());
    println!("Synced {} entries.", entries.len());

    Ok(())
}
