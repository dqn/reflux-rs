//! CLI argument parsing tests.
//!
//! These tests verify that command-line arguments are parsed correctly
//! without actually executing the commands (which would require the game process).

use clap::Parser;

// Re-create Args structure for testing since it's not publicly exported
#[derive(Parser)]
#[command(name = "reflux")]
struct Args {
    #[arg(long, value_name = "FILE")]
    offsets_file: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    FindOffsets {
        #[arg(short, long, default_value = "offsets.txt")]
        output: String,
        #[arg(long)]
        pid: Option<u32>,
    },
    Status {
        #[arg(long, value_name = "FILE")]
        offsets_file: Option<String>,
        #[arg(long)]
        pid: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    Hexdump {
        #[arg(long)]
        address: String,
        #[arg(long, default_value = "256")]
        size: usize,
        #[arg(long)]
        ascii: bool,
        #[arg(long)]
        pid: Option<u32>,
    },
    Offset {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
    Export {
        #[arg(long, short)]
        output: Option<String>,
        #[arg(long, short, value_enum, default_value = "tsv")]
        format: ExportFormat,
        #[arg(long)]
        pid: Option<u32>,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum ExportFormat {
    Tsv,
    Json,
}

#[test]
fn test_parse_no_args() {
    let args = Args::try_parse_from(["reflux"]).unwrap();
    assert!(args.command.is_none());
    assert!(args.offsets_file.is_none());
}

#[test]
fn test_parse_find_offsets() {
    let args = Args::try_parse_from(["reflux", "find-offsets"]).unwrap();
    match args.command {
        Some(Command::FindOffsets { output, pid }) => {
            assert_eq!(output, "offsets.txt");
            assert!(pid.is_none());
        }
        _ => panic!("Expected FindOffsets command"),
    }
}

#[test]
fn test_parse_find_offsets_with_output() {
    let args = Args::try_parse_from(["reflux", "find-offsets", "-o", "custom.txt"]).unwrap();
    match args.command {
        Some(Command::FindOffsets { output, .. }) => {
            assert_eq!(output, "custom.txt");
        }
        _ => panic!("Expected FindOffsets command"),
    }
}

#[test]
fn test_parse_status_with_json() {
    let args = Args::try_parse_from(["reflux", "status", "--json"]).unwrap();
    match args.command {
        Some(Command::Status { json, .. }) => {
            assert!(json);
        }
        _ => panic!("Expected Status command"),
    }
}

#[test]
fn test_parse_hexdump() {
    let args =
        Args::try_parse_from(["reflux", "hexdump", "--address", "0x1000", "--size", "512"]).unwrap();
    match args.command {
        Some(Command::Hexdump {
            address,
            size,
            ascii,
            ..
        }) => {
            assert_eq!(address, "0x1000");
            assert_eq!(size, 512);
            assert!(!ascii);
        }
        _ => panic!("Expected Hexdump command"),
    }
}

#[test]
fn test_parse_hexdump_with_ascii() {
    let args = Args::try_parse_from([
        "reflux", "hexdump", "--address", "0x1000", "--ascii",
    ])
    .unwrap();
    match args.command {
        Some(Command::Hexdump { ascii, .. }) => {
            assert!(ascii);
        }
        _ => panic!("Expected Hexdump command"),
    }
}

#[test]
fn test_parse_offset() {
    let args =
        Args::try_parse_from(["reflux", "offset", "--from", "0x1000", "--to", "0x2000"]).unwrap();
    match args.command {
        Some(Command::Offset { from, to }) => {
            assert_eq!(from, "0x1000");
            assert_eq!(to, "0x2000");
        }
        _ => panic!("Expected Offset command"),
    }
}

#[test]
fn test_parse_export_default_format() {
    let args = Args::try_parse_from(["reflux", "export"]).unwrap();
    match args.command {
        Some(Command::Export { format, output, .. }) => {
            assert!(output.is_none());
            assert!(matches!(format, ExportFormat::Tsv));
        }
        _ => panic!("Expected Export command"),
    }
}

#[test]
fn test_parse_export_json_format() {
    let args = Args::try_parse_from(["reflux", "export", "-f", "json", "-o", "scores.json"]).unwrap();
    match args.command {
        Some(Command::Export { format, output, .. }) => {
            assert!(matches!(format, ExportFormat::Json));
            assert_eq!(output, Some("scores.json".to_string()));
        }
        _ => panic!("Expected Export command"),
    }
}

#[test]
fn test_parse_global_offsets_file() {
    let args = Args::try_parse_from(["reflux", "--offsets-file", "my-offsets.txt"]).unwrap();
    assert_eq!(args.offsets_file, Some("my-offsets.txt".to_string()));
}

#[test]
fn test_parse_with_pid() {
    let args = Args::try_parse_from(["reflux", "status", "--pid", "12345"]).unwrap();
    match args.command {
        Some(Command::Status { pid, .. }) => {
            assert_eq!(pid, Some(12345));
        }
        _ => panic!("Expected Status command"),
    }
}

#[test]
fn test_invalid_command_fails() {
    let result = Args::try_parse_from(["reflux", "invalid-command"]);
    assert!(result.is_err());
}

#[test]
fn test_missing_required_arg_fails() {
    // hexdump requires --address
    let result = Args::try_parse_from(["reflux", "hexdump"]);
    assert!(result.is_err());
}
