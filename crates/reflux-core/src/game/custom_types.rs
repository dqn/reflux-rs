use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::Result;

/// Custom unlock type overrides for specific songs
#[derive(Debug, Clone, Default)]
pub struct CustomTypes {
    types: HashMap<String, String>,
    version: String,
}

impl CustomTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load custom types from file
    /// Format: first line is version (YYYYMMDD), subsequent lines are "songid,label"
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// Parse custom types from string content
    pub fn parse(content: &str) -> Result<Self> {
        let mut types = HashMap::new();
        let mut version = String::new();
        let mut first_line = true;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // First line is version string
            if first_line {
                version = line.to_string();
                first_line = false;
                continue;
            }

            // Skip lines without comma (invalid format)
            if !line.contains(',') {
                continue;
            }

            if let Some((song_id, label)) = line.split_once(',') {
                types.insert(song_id.trim().to_string(), label.trim().to_string());
            }
        }

        Ok(Self { types, version })
    }

    /// Get custom label for a song ID
    pub fn get(&self, song_id: &str) -> Option<&str> {
        self.types.get(song_id).map(|s| s.as_str())
    }

    /// Check if a song has a custom type override
    pub fn contains(&self, song_id: &str) -> bool {
        self.types.contains_key(song_id)
    }

    /// Get the version string
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Get all custom types
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.types.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_custom_types() {
        let content = r#"20240101
01000,Event
02000,Default
03000,Special
"#;
        let types = CustomTypes::parse(content).unwrap();

        assert_eq!(types.version(), "20240101");
        assert_eq!(types.get("01000"), Some("Event"));
        assert_eq!(types.get("02000"), Some("Default"));
        assert_eq!(types.get("03000"), Some("Special"));
        assert_eq!(types.get("99999"), None);
    }

    #[test]
    fn test_empty_custom_types() {
        let types = CustomTypes::parse("").unwrap();
        assert!(types.version().is_empty());
        assert!(types.get("01000").is_none());
    }
}
