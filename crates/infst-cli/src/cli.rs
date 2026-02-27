//! CLI argument definitions for infst.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "infst")]
#[command(about = "INFINITAS score tracker", version)]
pub struct Args {
    /// Load offsets from file (skip automatic detection)
    #[arg(long, value_name = "FILE")]
    pub offsets_file: Option<String>,

    /// API endpoint URL
    #[arg(long, env = "INFST_API_ENDPOINT")]
    pub api_endpoint: Option<String>,

    /// API token
    #[arg(long, env = "INFST_API_TOKEN")]
    pub api_token: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
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
    /// Login to the infst web service
    Login {
        /// API endpoint URL
        #[arg(
            long,
            env = "INFST_API_ENDPOINT",
            default_value = "https://infst.oidehosp.me"
        )]
        endpoint: String,
    },
    /// Sync all play data to the web service
    Sync {
        /// API endpoint URL
        #[arg(long, env = "INFST_API_ENDPOINT")]
        endpoint: Option<String>,
        /// API token
        #[arg(long, env = "INFST_API_TOKEN")]
        token: Option<String>,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
    },
    /// Navigate to a song on the select screen
    Navigate {
        /// Target song (fuzzy search query)
        target: String,
        /// Difficulty (SPN, SPH, SPA, etc.)
        #[arg(long, short)]
        difficulty: Option<String>,
        /// Maximum navigation steps
        #[arg(long, default_value = "3000")]
        max_steps: u32,
        /// Key press delay in ms
        #[arg(long, default_value = "80")]
        key_delay: u64,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
    },
    /// Launch INFINITAS in borderless window mode
    Launch {
        /// bm2dxinf:// URI to launch the game
        #[arg(long)]
        url: Option<String>,
        /// Process ID (skip automatic detection)
        #[arg(long)]
        pid: Option<u32>,
        /// Timeout in seconds for process detection
        #[arg(long, default_value = "120")]
        timeout: u64,
    },
    /// Upload tracker data to the web service
    Upload {
        /// Tracker TSV file path
        #[arg(long, short = 't', default_value = "tracker.tsv")]
        tracker: String,
        /// Title mapping JSON file path
        #[arg(long, short = 'm', default_value = "title-mapping.json")]
        mapping: String,
        /// API endpoint URL
        #[arg(long, env = "INFST_API_ENDPOINT")]
        endpoint: Option<String>,
        /// API token
        #[arg(long, env = "INFST_API_TOKEN")]
        token: Option<String>,
    },
}

#[derive(Clone, clap::ValueEnum)]
pub enum ExportFormat {
    Tsv,
    Json,
}

#[derive(Subcommand)]
pub enum ValidateTarget {
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
