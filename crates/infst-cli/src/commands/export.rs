//! Export command for exporting play data.

use anyhow::Result;
use infst::{
    MemoryReader, ScoreMap, fetch_song_database, generate_tracker_json, generate_tracker_tsv,
    get_unlock_states,
};

use crate::cli::ExportFormat;
use crate::cli_utils;

/// Export all play data
pub fn run(output: Option<&str>, format: ExportFormat, pid: Option<u32>) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    eprintln!("infst {} - Export Mode", current_version);

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

    // Load unlock data
    eprintln!("Loading unlock data...");
    let unlock_db = get_unlock_states(&reader, offsets.unlock_data, &song_db)?;
    eprintln!("Loaded {} unlock entries", unlock_db.len());

    // Load score map
    eprintln!("Loading score data...");
    let score_map = ScoreMap::load_from_memory(&reader, offsets.data_map, &song_db)?;
    eprintln!("Loaded {} score entries", score_map.len());

    // Generate output based on format
    let content = match format {
        ExportFormat::Tsv => generate_tracker_tsv(&song_db, &unlock_db, &score_map),
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
