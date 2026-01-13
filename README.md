# Reflux-RS

A Rust reimplementation of [Reflux](https://github.com/olji/Reflux), a score tracker for beatmania IIDX INFINITAS.

[日本語版はこちら](README.ja.md)

## Features

- **Memory Reading**: Reads game data directly from the INFINITAS process
- **Score Tracking**: Records play results including judgments, scores, and clear lamps
- **Auto Offset Search**: Automatically finds memory offsets when game updates
- **Local Storage**: Saves scores to TSV/JSON files
- **Remote Sync**: Syncs scores to remote servers
- **Kamaitachi Integration**: Supports [Kamaitachi](https://kamai.tachi.ac/) score submission
- **OBS Streaming**: Outputs current song info and play state to text files for OBS

## Requirements

- Windows (uses ReadProcessMemory API)
- Rust 1.85+ (Edition 2024)
- beatmania IIDX INFINITAS

## Installation

### From Source

```bash
git clone https://github.com/dqn/reflux-rs.git
cd reflux-rs
cargo build --release
```

The binary will be at `target/release/reflux.exe`.

## Usage

```bash
# Run with default settings
reflux

# Specify config and offsets files
reflux --config config.ini --offsets offsets.txt

# Show help
reflux --help
```

### Command Line Options

| Option | Default | Description |
|--------|---------|-------------|
| `-c, --config` | `config.ini` | Path to configuration file |
| `-o, --offsets` | `offsets.txt` | Path to offsets file |
| `-t, --tracker` | `tracker.db` | Path to tracker database |

## Configuration

Create a `config.ini` file:

```ini
[Update]
updatefiles = true
updateserver = https://example.com

[Record]
saveremote = false
savelocal = true
savejson = true
savelatestjson = false
savelatesttxt = true

[RemoteRecord]
serveraddress = https://example.com
apikey = your-api-key

[LocalRecord]
songinfo = true
chartdetails = true
resultdetails = true
judge = true
settings = true

[Livestream]
playstate = true
marquee = true
fullsonginfo = false
marqueeidletext = INFINITAS

[Debug]
outputdb = false
```

## Offsets File

The `offsets.txt` file contains memory addresses for reading game data:

```
P2D:J:B:A:2025010100
songList = 0x12345678
dataMap = 0x12345678
judgeData = 0x12345678
playData = 0x12345678
playSettings = 0x12345678
unlockData = 0x12345678
currentSong = 0x12345678
```

When the game updates and offsets change, Reflux will attempt to automatically find new offsets.

## Project Structure

```
reflux-rs/
├── Cargo.toml              # Workspace configuration
├── crates/
│   ├── reflux-core/        # Core library
│   │   └── src/
│   │       ├── config/     # INI configuration parser
│   │       ├── game/       # Game data structures
│   │       ├── memory/     # Windows API wrappers
│   │       ├── network/    # HTTP client, Kamaitachi API
│   │       ├── offset/     # Offset management
│   │       ├── storage/    # Local persistence
│   │       ├── stream/     # OBS streaming output
│   │       └── reflux.rs   # Main tracker logic
│   │
│   └── reflux-cli/         # CLI application
│       └── src/main.rs
```

## Output Files

### Session Files

Play data is saved to `sessions/Session_YYYY_MM_DD_HH_MM_SS.tsv`.

### Streaming Files (for OBS)

| File | Description |
|------|-------------|
| `playstate.txt` | Current state: `menu`, `play`, or `off` |
| `marquee.txt` | Current song title or status |
| `latest.txt` | Latest play result |
| `latest-grade.txt` | Latest grade (AAA, AA, etc.) |
| `latest-lamp.txt` | Latest clear lamp |

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Run with logging
RUST_LOG=reflux=debug cargo run

# Check code quality
cargo clippy
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Credits

- Original [Reflux](https://github.com/olji/Reflux) by olji
- [Kamaitachi/Tachi](https://github.com/zkrising/Tachi) for score tracking platform
