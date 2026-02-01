# reflux-rs

[[日本語](README.ja.md)]

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Release](https://img.shields.io/github/v/release/dqn/reflux-rs)](https://github.com/dqn/reflux-rs/releases)

A real-time score tracker for beatmania IIDX INFINITAS.

This is a Rust port of the original [Reflux](https://github.com/olji/Reflux) (C#).

## Features

- Automatically tracks play data in real-time
- Exports scores in TSV/JSON format

## Requirements

- Windows only
- beatmania IIDX INFINITAS installed

## Installation

1. Download `reflux.exe` from [GitHub Releases](https://github.com/dqn/reflux-rs/releases)
2. Place the executable anywhere you like

## Usage

### Tracking

Run with INFINITAS open:

```bash
reflux
```

Your plays are automatically recorded while the tracker is running.

### Export Data

Export all your play data (scores, lamps, miss counts, DJ points, etc.):

```bash
# Export to TSV (default)
reflux export -o scores.tsv

# Export to JSON
reflux export -o scores.json -f json

# Output to stdout
reflux export
```

#### Options

| Option | Description |
|--------|-------------|
| `-o, --output` | Output file path (stdout if omitted) |
| `-f, --format` | Output format: `tsv` (default) / `json` |

## License

[MIT License](LICENSE)

## Credits

Based on [Reflux](https://github.com/olji/Reflux) by olji.
