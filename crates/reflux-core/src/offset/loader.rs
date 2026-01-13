use tracing::warn;

use crate::error::{Error, Result};
use crate::offset::OffsetsCollection;
use std::fs;
use std::path::Path;

pub fn load_offsets<P: AsRef<Path>>(path: P) -> Result<OffsetsCollection> {
    let content = fs::read_to_string(&path)?;
    parse_offsets(&content)
}

pub fn save_offsets<P: AsRef<Path>>(path: P, offsets: &OffsetsCollection) -> Result<()> {
    let content = format_offsets(offsets);
    fs::write(path, content)?;
    Ok(())
}

fn parse_offsets(content: &str) -> Result<OffsetsCollection> {
    let mut offsets = OffsetsCollection::default();
    let mut lines = content.lines();

    // First line is version
    if let Some(version) = lines.next() {
        offsets.version = version.trim().to_string();
    }

    // Parse key = value pairs
    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim();

            let parsed_value = parse_hex_value(value)?;

            match key.as_str() {
                "songlist" => offsets.song_list = parsed_value,
                "datamap" => offsets.data_map = parsed_value,
                "judgedata" => offsets.judge_data = parsed_value,
                "playdata" => offsets.play_data = parsed_value,
                "playsettings" => offsets.play_settings = parsed_value,
                "unlockdata" => offsets.unlock_data = parsed_value,
                "currentsong" => offsets.current_song = parsed_value,
                _ => {
                    warn!("Unknown offset key: '{}' (value: {})", key, value);
                }
            }
        }
    }

    Ok(offsets)
}

fn parse_hex_value(value: &str) -> Result<u64> {
    let value = value.trim();
    // Strip hex prefix (case-insensitive), only once
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);

    u64::from_str_radix(value, 16)
        .map_err(|e| Error::InvalidOffset(format!("Failed to parse '{}': {}", value, e)))
}

fn format_offsets(offsets: &OffsetsCollection) -> String {
    let mut lines = Vec::new();

    lines.push(offsets.version.clone());
    lines.push(format!("songList = {:#x}", offsets.song_list));
    lines.push(format!("dataMap = {:#x}", offsets.data_map));
    lines.push(format!("judgeData = {:#x}", offsets.judge_data));
    lines.push(format!("playData = {:#x}", offsets.play_data));
    lines.push(format!("playSettings = {:#x}", offsets.play_settings));
    lines.push(format!("unlockData = {:#x}", offsets.unlock_data));
    lines.push(format!("currentSong = {:#x}", offsets.current_song));

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_offsets() {
        let content = r#"P2D:J:B:A:2025101500
songList = 0x12345678
judgeData = 0xABCDEF00
playData = 0x87654321
"#;
        let offsets = parse_offsets(content).unwrap();

        assert_eq!(offsets.version, "P2D:J:B:A:2025101500");
        assert_eq!(offsets.song_list, 0x12345678);
        assert_eq!(offsets.judge_data, 0xABCDEF00);
        assert_eq!(offsets.play_data, 0x87654321);
    }

    #[test]
    fn test_format_offsets() {
        let offsets = OffsetsCollection {
            version: "P2D:J:B:A:2025101500".to_string(),
            song_list: 0x1000,
            judge_data: 0x2000,
            ..Default::default()
        };

        let formatted = format_offsets(&offsets);
        assert!(formatted.contains("P2D:J:B:A:2025101500"));
        assert!(formatted.contains("songList = 0x1000"));
    }
}
